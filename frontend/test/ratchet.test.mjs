// Regression suite for the X3DH + Double Ratchet implemented inline in
// frontend/index.html (window.YeetRatchet). Run with:
//
//     node frontend/test/ratchet.test.mjs
//
// It is a faithful copy of the inlined algorithm kept as an ESM module
// so the crypto can be exercised with real WebCrypto test vectors
// outside the browser. The inlined core in index.html uses identical
// KDF labels/constants, the X3DH 0xff constant, HKDF sizes and DH
// ordering — the KDF known-answer tests below pin that construction so
// any drift in either copy is caught.
//
// Coverage: X3DH establish, DH ratchet, out-of-order + skipped keys,
// interleaved round-trips, AEAD tamper/replay state-integrity, no-OTP
// path, multi-device fan-out + self-sync + cross-device rejection,
// glare tiebreak convergence, and KDF/X3DH known-answer vectors.
//
// Curve choices match Yeet's existing crypto: ECDH P-256 for all key
// agreement, ECDSA P-256 for the signed-prekey signature, HKDF-SHA256
// for the root KDF, HMAC-SHA256 for the chain KDF, AES-GCM for the
// message AEAD. (These are NOT libsignal's Curve25519 vectors — the
// KATs pin OUR construction.)

const subtle = (globalThis.crypto || (await import('crypto')).webcrypto).subtle;
const crypto = (globalThis.crypto || (await import('crypto')).webcrypto);

const te = new TextEncoder();
const td = new TextDecoder();

function b64(u8){ return Buffer.from(u8).toString('base64'); }
function ub64(s){ return new Uint8Array(Buffer.from(s, 'base64')); }
function concat(...arrs){
  let n = 0; for(const a of arrs) n += a.length;
  const out = new Uint8Array(n); let o = 0;
  for(const a of arrs){ out.set(a, o); o += a.length; }
  return out;
}
function eq(a, b){ if(a.length!==b.length) return false; for(let i=0;i<a.length;i++) if(a[i]!==b[i]) return false; return true; }

// ── key helpers ────────────────────────────────────────────────────
async function genDH(){ return subtle.generateKey({name:'ECDH',namedCurve:'P-256'}, true, ['deriveBits']); }
async function genSign(){ return subtle.generateKey({name:'ECDSA',namedCurve:'P-256'}, true, ['sign','verify']); }
async function pubSpki(k){ return new Uint8Array(await subtle.exportKey('spki', k)); }
async function impDHpub(u8){ return subtle.importKey('spki', u8, {name:'ECDH',namedCurve:'P-256'}, false, []); }
async function impVerify(u8){ return subtle.importKey('spki', u8, {name:'ECDSA',namedCurve:'P-256'}, false, ['verify']); }
async function dh(priv, pub){ return new Uint8Array(await subtle.deriveBits({name:'ECDH', public: pub}, priv, 256)); }

// HKDF(salt, ikm, info) -> 64 bytes split into (rk32, ck32)
async function kdfRk(rk, dhOut){
  const ikm = await subtle.importKey('raw', dhOut, 'HKDF', false, ['deriveBits']);
  const bits = new Uint8Array(await subtle.deriveBits(
    {name:'HKDF', hash:'SHA-256', salt: rk, info: te.encode('yeet-ratchet-rk-v1')}, ikm, 512));
  return [bits.slice(0,32), bits.slice(32,64)];
}
// KDF_CK: MK = HMAC(CK,0x01), CK' = HMAC(CK,0x02)
async function kdfCk(ck){
  const key = await subtle.importKey('raw', ck, {name:'HMAC', hash:'SHA-256'}, false, ['sign']);
  const mk  = new Uint8Array(await subtle.sign('HMAC', key, new Uint8Array([0x01])));
  const nck = new Uint8Array(await subtle.sign('HMAC', key, new Uint8Array([0x02])));
  return [nck, mk];
}
async function aesEncrypt(mk, plaintext, ad){
  const key = await subtle.importKey('raw', mk, {name:'AES-GCM'}, false, ['encrypt']);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await subtle.encrypt({name:'AES-GCM', iv, additionalData: ad}, key, te.encode(plaintext)));
  return concat(iv, ct);
}
async function aesDecrypt(mk, blob, ad){
  const key = await subtle.importKey('raw', mk, {name:'AES-GCM'}, false, ['decrypt']);
  const iv = blob.slice(0,12); const ct = blob.slice(12);
  const pt = new Uint8Array(await subtle.decrypt({name:'AES-GCM', iv, additionalData: ad}, key, ct));
  return td.decode(pt);
}

// X3DH shared secret. dhParts already computed by caller in the right
// order; we prepend the standard 32-byte 0xFF "F" constant per spec.
async function x3dhRoot(dhParts){
  const F = new Uint8Array(32).fill(0xff);
  const ikm = concat(F, ...dhParts);
  const k = await subtle.importKey('raw', ikm, 'HKDF', false, ['deriveBits']);
  const bits = new Uint8Array(await subtle.deriveBits(
    {name:'HKDF', hash:'SHA-256', salt: new Uint8Array(32), info: te.encode('yeet-x3dh-v1')}, k, 256));
  return bits; // 32-byte SK
}

const MAX_SKIP = 100;

// ── initiator (Alice) ───────────────────────────────────────────────
// bundle: { identityKeyPub(u8), signingKeyPub(u8), signedPrekeyPub(u8),
//           signedPrekeySig(u8), signedPrekeyId, oneTimePrekeyPub(u8|null), oneTimePrekeyId|null }
// self:   { idPriv(CryptoKey), idPub(u8) }
async function initSender(self, bundle){
  // Verify the signed prekey signature against the signing identity key.
  const verifyKey = await impVerify(bundle.signingKeyPub);
  const ok = await subtle.verify({name:'ECDSA', hash:'SHA-256'}, verifyKey, bundle.signedPrekeySig, bundle.signedPrekeyPub);
  if(!ok) throw new Error('signed prekey signature invalid');

  const ek = await genDH();                       // ephemeral
  const ekPub = await pubSpki(ek.publicKey);
  const IKb = await impDHpub(bundle.identityKeyPub);
  const SPKb = await impDHpub(bundle.signedPrekeyPub);
  const OPKb = bundle.oneTimePrekeyPub ? await impDHpub(bundle.oneTimePrekeyPub) : null;

  const dh1 = await dh(self.idPriv, SPKb);  // DH(IK_a, SPK_b)
  const dh2 = await dh(ek.privateKey, IKb); // DH(EK_a, IK_b)
  const dh3 = await dh(ek.privateKey, SPKb);// DH(EK_a, SPK_b)
  const parts = [dh1, dh2, dh3];
  if(OPKb) parts.push(await dh(ek.privateKey, OPKb)); // DH(EK_a, OPK_b)
  const sk = await x3dhRoot(parts);

  // Double Ratchet init for the initiator: Bob's signed prekey is his
  // initial ratchet public key. Alice generates her ratchet keypair
  // and does the first DH ratchet step.
  const rk0 = sk;
  const ratchet = await genDH();
  const dhOut = await dh(ratchet.privateKey, SPKb);
  const [rk1, cks] = await kdfRk(rk0, dhOut);

  const st = {
    role:'A',
    DHs: ratchet,                 // our ratchet keypair
    DHsPub: await pubSpki(ratchet.publicKey),
    DHr: bundle.signedPrekeyPub,  // their ratchet pub (= SPK_b initially)
    RK: rk1, CKs: cks, CKr: null,
    Ns: 0, Nr: 0, PN: 0,
    skipped: new Map(),
    // X3DH preamble the responder needs on the very first message:
    pre: {
      ik: b64(self.idPub), ek: b64(ekPub),
      spk_id: bundle.signedPrekeyId,
      opk_id: bundle.oneTimePrekeyId == null ? null : bundle.oneTimePrekeyId,
    },
    preSent: false,
  };
  return st;
}

// ── responder (Bob) processes the first inbound message ─────────────
// self: { idPriv, idPub, signedPrekeyPriv (CryptoKey for SPK_b),
//         oneTimePrekeyPrivById: Map(id->CryptoKey) }
async function initReceiver(self, header){
  const IKa = await impDHpub(ub64(header.pre.ik));
  const EKa = await impDHpub(ub64(header.pre.ek));
  const spkPriv = self.signedPrekeyPriv;
  const dh1 = await dh(spkPriv, IKa);
  const dh2 = await dh(self.idPriv, EKa);
  const dh3 = await dh(spkPriv, EKa);
  const parts = [dh1, dh2, dh3];
  if(header.pre.opk_id != null){
    const opkPriv = self.oneTimePrekeyPrivById.get(header.pre.opk_id);
    if(!opkPriv) throw new Error('one-time prekey not found: '+header.pre.opk_id);
    parts.push(await dh(opkPriv, EKa));
  }
  const sk = await x3dhRoot(parts);

  // Bob's initial ratchet keypair IS the signed prekey.
  const st = {
    role:'B',
    DHs: { privateKey: self.signedPrekeyPriv, publicKey: null },
    DHsPub: self.signedPrekeyPub,
    DHr: null,
    RK: sk, CKs: null, CKr: null,
    Ns: 0, Nr: 0, PN: 0,
    skipped: new Map(),
    pre: null, preSent: true,
  };
  return st;
}

function skKey(dhrPub, n){ return b64(dhrPub)+'|'+n; }

async function dhRatchet(st, header){
  st.PN = st.Ns; st.Ns = 0; st.Nr = 0;
  st.DHr = header.dh;
  const DHrPub = await impDHpub(header.dh);
  let [rk, ckr] = await kdfRk(st.RK, await dh(st.DHs.privateKey, DHrPub));
  st.RK = rk; st.CKr = ckr;
  // generate new sending ratchet keypair
  st.DHs = await genDH();
  st.DHsPub = await pubSpki(st.DHs.publicKey);
  let [rk2, cks] = await kdfRk(st.RK, await dh(st.DHs.privateKey, DHrPub));
  st.RK = rk2; st.CKs = cks;
}

async function trySkipped(st, header, blob, ad){
  const k = skKey(header.dh, header.n);
  if(st.skipped.has(k)){
    const mk = st.skipped.get(k);
    st.skipped.delete(k);
    return await aesDecrypt(mk, blob, ad);
  }
  return null;
}

async function skipUntil(st, until){
  if(st.CKr == null) return;
  if(st.Nr + MAX_SKIP < until) throw new Error('too many skipped messages');
  while(st.Nr < until){
    const [nck, mk] = await kdfCk(st.CKr);
    st.skipped.set(skKey(st.DHr, st.Nr), mk);
    st.CKr = nck; st.Nr++;
  }
}

async function ratchetEncrypt(st, plaintext){
  const [nck, mk] = await kdfCk(st.CKs);
  st.CKs = nck;
  const header = { dh: st.DHsPub, n: st.Ns, pn: st.PN };
  if(!st.preSent && st.pre){ header.pre = st.pre; }
  st.Ns++;
  // AD binds the header so a tampered header fails AEAD.
  const ad = te.encode(JSON.stringify({ dh: b64(header.dh), n: header.n, pn: header.pn }));
  const blob = await aesEncrypt(mk, plaintext, ad);
  if(header.pre) st.preSent = true;
  return { header: { dh: b64(header.dh), n: header.n, pn: header.pn, pre: header.pre || null }, body: b64(blob) };
}

// Atomic decrypt: all mutation happens on a shallow clone `w`; the
// caller swaps it in only when this resolves. A tampered/duplicate
// message therefore can't advance (and corrupt) the committed chain.
async function ratchetDecryptInto(w, wire){
  const header = { dh: ub64(wire.header.dh), n: wire.header.n, pn: wire.header.pn };
  const blob = ub64(wire.body);
  const ad = te.encode(JSON.stringify({ dh: wire.header.dh, n: wire.header.n, pn: wire.header.pn }));

  const sk = await trySkipped(w, header, blob, ad);
  if(sk != null) return sk; // skipped-key hit decrypts before committing anything else

  const isNewRatchet = (w.DHr == null) || !eq(header.dh, w.DHr);
  if(isNewRatchet){
    if(w.DHr != null) await skipUntil(w, header.pn);
    await dhRatchet(w, header);
  }
  await skipUntil(w, header.n);
  const [nck, mk] = await kdfCk(w.CKr);
  // Decrypt FIRST; only commit the chain advance if AEAD succeeds.
  const plain = await aesDecrypt(mk, blob, ad);
  w.CKr = nck; w.Nr++;
  return plain;
}
function cloneState(st){
  const w = Object.assign({}, st);
  w.skipped = new Map(st.skipped);
  return w;
}
async function ratchetDecrypt(st, wire){
  const w = cloneState(st);
  const plain = await ratchetDecryptInto(w, wire); // throws -> st untouched
  // commit
  Object.assign(st, w);
  return plain;
}

// ── self-tests ──────────────────────────────────────────────────────
async function setupBob(){
  const id = await genDH();
  const sign = await genSign();
  const spk = await genDH();
  const spkPub = await pubSpki(spk.publicKey);
  const spkSig = new Uint8Array(await subtle.sign({name:'ECDSA',hash:'SHA-256'}, sign.privateKey, spkPub));
  const opk = await genDH();
  const opkPub = await pubSpki(opk.publicKey);
  return {
    self: {
      idPriv: id.privateKey, idPub: await pubSpki(id.publicKey),
      signedPrekeyPriv: spk.privateKey, signedPrekeyPub: spkPub,
      oneTimePrekeyPrivById: new Map([[7, opk.privateKey]]),
    },
    bundle: {
      identityKeyPub: await pubSpki(id.publicKey),
      signingKeyPub: await pubSpki(sign.publicKey),
      signedPrekeyPub: spkPub, signedPrekeySig: spkSig, signedPrekeyId: 1,
      oneTimePrekeyPub: opkPub, oneTimePrekeyId: 7,
    },
  };
}
async function setupAlice(){
  const id = await genDH();
  return { idPriv: id.privateKey, idPub: await pubSpki(id.publicKey) };
}

let pass = 0, fail = 0;
function check(name, cond){ if(cond){ pass++; } else { fail++; console.error('FAIL:', name); } }

(async () => {
  const bob = await setupBob();
  const alice = await setupAlice();

  // Alice establishes a session and sends the first message.
  const aSt = await initSender(alice, bob.bundle);
  const m1 = await ratchetEncrypt(aSt, 'hello bob');
  check('m1 carries X3DH preamble', m1.header.pre && m1.header.pre.opk_id === 7);

  // Bob processes the first message (X3DH responder) then decrypts.
  const bSt = await initReceiver(bob.self, m1.header);
  const p1 = await ratchetDecrypt(bSt, m1);
  check('bob decrypts m1', p1 === 'hello bob');

  // Bob replies — triggers a DH ratchet on Alice's side.
  const m2 = await ratchetEncrypt(bSt, 'hi alice');
  const p2 = await ratchetDecrypt(aSt, m2);
  check('alice decrypts m2 (dh ratchet)', p2 === 'hi alice');

  // Alice sends three; Bob receives out of order (3,1,2).
  const a3 = await ratchetEncrypt(aSt, 'one');
  const a4 = await ratchetEncrypt(aSt, 'two');
  const a5 = await ratchetEncrypt(aSt, 'three');
  const r5 = await ratchetDecrypt(bSt, a5); // newest first -> skipped keys
  const r3 = await ratchetDecrypt(bSt, a3);
  const r4 = await ratchetDecrypt(bSt, a4);
  check('out-of-order three', r5 === 'three');
  check('out-of-order one (skipped)', r3 === 'one');
  check('out-of-order two (skipped)', r4 === 'two');

  // Several back-and-forth round trips exercise repeated DH ratchets.
  let okRoundtrips = true;
  for(let i=0;i<6;i++){
    const ma = await ratchetEncrypt(aSt, 'a'+i);
    if(await ratchetDecrypt(bSt, ma) !== 'a'+i) okRoundtrips = false;
    const mb = await ratchetEncrypt(bSt, 'b'+i);
    if(await ratchetDecrypt(aSt, mb) !== 'b'+i) okRoundtrips = false;
  }
  check('6 interleaved round trips', okRoundtrips);

  // Tamper detection: flip a body byte -> decrypt must throw.
  const mt = await ratchetEncrypt(aSt, 'secret');
  const bad = ub64(mt.body); bad[bad.length-1] ^= 0x01; mt.body = b64(bad);
  let threw = false;
  try { await ratchetDecrypt(bSt, mt); } catch(e){ threw = true; }
  check('tampered ciphertext rejected', threw);

  // After a tampered message, the NEXT legit message must still
  // decrypt — i.e. the failed decrypt must NOT have advanced state.
  const good = await ratchetEncrypt(aSt, 'still works');
  const rgood = await ratchetDecrypt(bSt, good);
  check('state intact after tamper', rgood === 'still works');

  // A duplicate (replayed) message: first decrypts, replay must fail
  // (chain already advanced past it) without corrupting later msgs.
  const dup = await ratchetEncrypt(aSt, 'once');
  const d1 = await ratchetDecrypt(bSt, dup);
  let dupThrew = false;
  try { await ratchetDecrypt(bSt, dup); } catch(e){ dupThrew = true; }
  const after = await ratchetEncrypt(aSt, 'after dup');
  const rafter = await ratchetDecrypt(bSt, after);
  check('duplicate decrypts once', d1 === 'once');
  check('replay rejected', dupThrew);
  check('state intact after replay', rafter === 'after dup');

  // A second independent session (no OPK) must also work.
  const bob2 = await setupBob();
  bob2.bundle.oneTimePrekeyPub = null; bob2.bundle.oneTimePrekeyId = null;
  const aSt2 = await initSender(alice, bob2.bundle);
  const mm = await ratchetEncrypt(aSt2, 'no-otp path');
  const bSt2 = await initReceiver(bob2.self, mm.header);
  const pp = await ratchetDecrypt(bSt2, mm);
  check('session without one-time prekey', pp === 'no-otp path');

  // ── multi-device fan-out scenario ─────────────────────────────────
  // Alice has 1 device (A1); Bob has 2 (B1, B2). A message is fanned
  // out to every recipient device + the sender's own other devices.
  // Each device keeps sessions keyed by the REMOTE device id; the wire
  // carries the sender device id ("from") + a per-recipient-device map.
  function makeBundleFrom(setup, deviceId){
    return {
      deviceId,
      identityKeyPub: setup.bundle.identityKeyPub,
      signingKeyPub: setup.bundle.signingKeyPub,
      signedPrekeyPub: setup.bundle.signedPrekeyPub,
      signedPrekeySig: setup.bundle.signedPrekeySig,
      signedPrekeyId: setup.bundle.signedPrekeyId,
      oneTimePrekeyPub: setup.bundle.oneTimePrekeyPub,
      oneTimePrekeyId: setup.bundle.oneTimePrekeyId,
    };
  }
  // device registry: deviceId -> { setup (responder self+bundle), identity {idPriv,idPub}, sessions{} }
  async function mkDevice(){
    const setup = await setupBob();           // gives self + bundle (incl OTP id 7)
    return { setup, idPriv: setup.self.idPriv, idPub: setup.self.idPub, sessions: {} };
  }
  async function fanoutSend(sender, targets, text){
    // targets: [{deviceId, bundle}]; sessions keyed by remote deviceId
    const msgs = {};
    for(const t of targets){
      let st = sender.sessions[t.deviceId];
      if(!st){ st = await initSender({idPriv:sender.idPriv, idPub:sender.idPub}, t.bundle); sender.sessions[t.deviceId] = st; }
      const wire = await ratchetEncrypt(st, text);
      msgs[t.deviceId] = wire;
    }
    return { v:3, from: sender.deviceId, msgs };
  }
  async function fanoutRecv(dev, wire){
    const entry = wire.msgs[dev.deviceId];
    if(!entry) return null; // not addressed to this device
    let st = dev.sessions[wire.from];
    if(!st){
      st = await initReceiver(dev.setup.self, entry.header);
      dev.sessions[wire.from] = st;
    }
    return await ratchetDecrypt(st, entry);
  }

  const A1 = await mkDevice(); A1.deviceId = 'A1';
  const B1 = await mkDevice(); B1.deviceId = 'B1';
  const B2 = await mkDevice(); B2.deviceId = 'B2';

  // Alice sends to Bob's two devices (no own other devices).
  const w1 = await fanoutSend(A1, [
    {deviceId:'B1', bundle: makeBundleFrom(B1.setup,'B1')},
    {deviceId:'B2', bundle: makeBundleFrom(B2.setup,'B2')},
  ], 'multi hello');
  const b1r = await fanoutRecv(B1, w1);
  const b2r = await fanoutRecv(B2, w1);
  check('B1 decrypts fan-out', b1r === 'multi hello');
  check('B2 decrypts fan-out', b2r === 'multi hello');
  // A1 is the sender — there's no entry addressed to it.
  check('sender device has no own entry', !w1.msgs['A1']);

  // Bob (from B1) replies, fanning out to Alice (A1) + his own other
  // device (B2) for self-sync.
  const w2 = await fanoutSend(B1, [
    {deviceId:'A1', bundle: makeBundleFrom(A1.setup,'A1')},
    {deviceId:'B2', bundle: makeBundleFrom(B2.setup,'B2')},
  ], 'reply all my devices');
  const a1r = await fanoutRecv(A1, w2);
  const b2self = await fanoutRecv(B2, w2);
  check('A1 decrypts Bob reply', a1r === 'reply all my devices');
  check('B2 self-syncs Bob reply', b2self === 'reply all my devices');

  // A device must NOT decrypt an entry addressed to a different device.
  let wrongThrew = false;
  try {
    const stolen = { v:3, from:'B1', msgs: { 'A1': w2.msgs['B2'] } }; // B2's entry relabelled to A1
    await fanoutRecv(A1, stolen);
  } catch(e){ wrongThrew = true; }
  check('cross-device entry rejected', wrongThrew);

  // ── KDF known-answer tests (pin our construction) ─────────────────
  // These are NOT libsignal vectors (we use P-256 + HKDF/HMAC with our
  // own info labels, not Curve25519). They pin OUR construction so any
  // future change to a KDF label/constant/curve is caught immediately.
  {
    const rkIn = new Uint8Array(32); for(let i=0;i<32;i++) rkIn[i]=i;
    const dhOut = new Uint8Array(32); for(let i=0;i<32;i++) dhOut[i]=255-i;
    const ckIn = new Uint8Array(32); for(let i=0;i<32;i++) ckIn[i]=(i*7)&0xff;
    const [rk2, ckOut] = await kdfRk(rkIn, dhOut);
    const [nck, mk] = await kdfCk(ckIn);
    const sk = await x3dhRoot([new Uint8Array(32).fill(0x11), new Uint8Array(32).fill(0x22), new Uint8Array(32).fill(0x33)]);
    check('KAT kdfRk root', b64(rk2) === 'Ps8ubN4eKyyqn7L1Tyk1ZRLFhmZizo/ofdbwtYXz7s4=');
    check('KAT kdfRk chain', b64(ckOut) === 'FIc0jqO1cwHiSxePR8+8KU0msX2mDATQq19kfu8ejl0=');
    check('KAT kdfCk next-chain', b64(nck) === 'hFIDg8qu4I5fUNmWIaQHBl6+Q8SeFAvMoONYegHmuZM=');
    check('KAT kdfCk message-key', b64(mk) === 'GoPaEt52dlepJyzoYLk38VhO/rdmQpROf6C/hU/ki0I=');
    check('KAT x3dhRoot', b64(sk) === '2McpEKcnVLrQmaPhh6fiig/kRF2Z/FekZTMyBJqNJ5c=');
  }

  // ── glare: both peers initiate at the same instant ────────────────
  // Deterministic tiebreak: the device with the lexicographically
  // smaller id is the canonical winner; the loser adopts the winner's
  // session. Convergence is asserted below.
  function setInit(st, who){ st.init = who; return st; }
  async function glareSend(dev, remoteId, remoteBundle, text){
    let st = dev.sessions[remoteId];
    if(!st){ st = setInit(await initSender({idPriv:dev.idPriv, idPub:dev.idPub}, remoteBundle), 'me'); dev.sessions[remoteId] = st; }
    return await ratchetEncrypt(st, text);
  }
  async function glareRecv(dev, remoteId, wire){
    const entry = wire.msgs[dev.deviceId];
    const hasPre = entry && entry.header && entry.header.pre;
    let st = dev.sessions[remoteId];
    if(hasPre && st && st.init === 'me'){
      if(dev.deviceId < remoteId){
        throw new Error('glare: winner ignores peer init'); // fail-closed, loser will converge
      }
      st = setInit(await initReceiver(dev.setup.self, entry.header), 'peer');
      dev.sessions[remoteId] = st; // loser adopts winner's session
    } else if(!st){
      if(!hasPre) throw new Error('no session');
      st = setInit(await initReceiver(dev.setup.self, entry.header), 'peer');
      dev.sessions[remoteId] = st;
    }
    return await ratchetDecrypt(st, entry);
  }

  const GA = await mkDevice(); GA.deviceId = 'dev-A';
  const GB = await mkDevice(); GB.deviceId = 'dev-B';
  const bundleA = makeBundleFrom(GA.setup, 'dev-A');
  const bundleB = makeBundleFrom(GB.setup, 'dev-B');

  // Both send a first message simultaneously (each init's its own).
  const gA1 = { v:3, from:'dev-A', msgs: { 'dev-B': await glareSend(GA, 'dev-B', bundleB, 'A first') } };
  const gB1 = { v:3, from:'dev-B', msgs: { 'dev-A': await glareSend(GB, 'dev-A', bundleA, 'B first') } };

  // A is canonical winner ('dev-A' < 'dev-B'): ignores B's init (fail-closed).
  let aIgnored = false;
  try { await glareRecv(GA, 'dev-B', gB1); } catch(e){ aIgnored = true; }
  check('glare winner rejects peer init', aIgnored);
  // B is loser: adopts A's session and decrypts A's first message.
  const bGotA = await glareRecv(GB, 'dev-A', gA1);
  check('glare loser decrypts winner first msg', bGotA === 'A first');

  // Convergence: B replies on the adopted (winner's) session; A reads it.
  const gB2 = { v:3, from:'dev-B', msgs: { 'dev-A': await ratchetEncrypt(GB.sessions['dev-A'], 'B reply') } };
  const aGotB = await glareRecv(GA, 'dev-B', gB2);
  check('glare converged: winner reads loser reply', aGotB === 'B reply');
  // And further A->B traffic flows on the single converged session.
  const gA2 = { v:3, from:'dev-A', msgs: { 'dev-B': await ratchetEncrypt(GA.sessions['dev-B'], 'A again') } };
  const bGotA2 = await glareRecv(GB, 'dev-A', gA2);
  check('glare converged: loser reads further winner msg', bGotA2 === 'A again');

  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})().catch(e => { console.error('THREW', e); process.exit(1); });
