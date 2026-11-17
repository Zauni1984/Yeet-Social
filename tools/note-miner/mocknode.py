# Fake node: serves a template with an easy target and fully validates the
# submitted block (header layout, merkle root, scrypt PoW, coinbase fields).
import json,hashlib,struct,sys,os
from http.server import BaseHTTPRequestHandler,HTTPServer
WITH_COMMIT = len(sys.argv)>1 and sys.argv[1]=='segwit'
sha256d=lambda b:hashlib.sha256(hashlib.sha256(b).digest()).digest()
PREV='38848c35052aa4b8e154a226de508ad07e2c35f4226bf39675f371bc26a32827'
TARGET='000fffff'+'f'*56
HEIGHT=4621933
# two fake non-witness transactions
TXS=[bytes.fromhex('01000000')+b'\x01'+os.urandom(32)+b'\x00\x00\x00\x00\x00\xff\xff\xff\xff\x01'+struct.pack('<Q',5)+b'\x01\x51'+b'\x00\x00\x00\x00' for _ in range(2)]
SCRIPT='76a914'+'11'*20+'88ac'
COMMIT='6a24aa21a9ed'+'22'*32
state={'accepted':0,'proposal':0}
def rv(b):
    def take(n):
        nonlocal b; x,b=b[:n],b[n:]; return x
    return take
def varint(buf,pos):
    n=buf[pos]
    if n<0xfd: return n,pos+1
    if n==0xfd: return struct.unpack_from('<H',buf,pos+1)[0],pos+3
    raise Exception('varint')
def check(blockhex,pow_check=True):
    b=bytes.fromhex(blockhex); h=b[:80]
    ver,=struct.unpack_from('<i',h,0)
    assert ver==0x20000000,'version'
    assert h[4:36]==bytes.fromhex(PREV)[::-1],'prev'
    bits,=struct.unpack_from('<I',h,72); assert bits==0x1f0fffff,'bits %x'%bits
    n,pos=varint(b,80); assert n==3,'txcount %d'%n
    # coinbase
    start=pos
    assert b[pos:pos+4]==b'\x01\x00\x00\x00'; pos+=4
    seg = b[pos:pos+2]==b'\x00\x01'
    assert seg==WITH_COMMIT,'segwit marker mismatch'
    if seg: pos+=2
    body_start=pos
    assert b[pos]==1; pos+=1
    assert b[pos:pos+32]==bytes(32) and b[pos+32:pos+36]==b'\xff'*4; pos+=36
    sl,pos=varint(b,pos); ss=b[pos:pos+sl]; pos+=sl
    hl=ss[0]; assert int.from_bytes(ss[1:1+hl],'little')==HEIGHT,'bip34 height'
    assert 2<=sl<=100,'scriptsig size'
    assert b[pos:pos+4]==b'\xff'*4; pos+=4
    no,pos=varint(b,pos); assert no==(2 if WITH_COMMIT else 1),'outputs'
    val,=struct.unpack_from('<Q',b,pos); pos+=8; assert val==10000+10,'value %d'%val
    l,pos=varint(b,pos); assert b[pos:pos+l].hex()==SCRIPT; pos+=l
    if WITH_COMMIT:
        v,=struct.unpack_from('<Q',b,pos); pos+=8; assert v==0
        l,pos=varint(b,pos); assert b[pos:pos+l].hex()==COMMIT; pos+=l
    body_end=pos
    if seg:
        assert b[pos:pos+2]==b'\x01\x20' and b[pos+2:pos+34]==bytes(32); pos+=34
    assert b[pos:pos+4]==bytes(4); pos+=4
    plain=b'\x01\x00\x00\x00'+b[body_start:body_end]+bytes(4)
    ids=[sha256d(plain)]+[sha256d(t) for t in TXS]
    assert b[pos:]==b''.join(TXS),'tx data'
    lvl=ids
    while len(lvl)>1:
        if len(lvl)%2: lvl.append(lvl[-1])
        lvl=[sha256d(lvl[i]+lvl[i+1]) for i in range(0,len(lvl),2)]
    assert h[36:68]==lvl[0],'merkle root'
    if pow_check:
        p=hashlib.scrypt(h,salt=h,n=1024,r=1,p=1,dklen=32)
        assert int.from_bytes(p,'little')<=int(TARGET,16),'pow'
class H(BaseHTTPRequestHandler):
    def log_message(self,*a): pass
    def do_POST(self):
        assert self.headers['Authorization'].startswith('Basic ')
        q=json.loads(self.rfile.read(int(self.headers['Content-Length'])));m=q['method'];p=q['params'];r=None;e=None
        if m=='getblockchaininfo': r={'chain':'main','blocks':HEIGHT-1,'difficulty':0.0177}
        elif m=='validateaddress': r={'isvalid':True,'scriptPubKey':SCRIPT}
        elif m=='getbestblockhash': r=PREV
        elif m=='getblocktemplate':
            if p and p[0].get('mode')=='proposal':
                try: check(p[0]['data'],False); state['proposal']+=1; r=None
                except AssertionError as x: r=str(x)
            else:
                r={'version':0x20000000,'previousblockhash':PREV,'target':TARGET,'bits':'1f0fffff','curtime':1795000000,'height':HEIGHT,'coinbasevalue':10010,
                   'transactions':[{'data':t.hex(),'txid':sha256d(t)[::-1].hex(),'hash':sha256d(t)[::-1].hex()} for t in TXS]}
                if WITH_COMMIT: r['default_witness_commitment']=COMMIT
        elif m=='submitblock':
            try: check(p[0]); state['accepted']+=1; r=None; print('MOCK accepted block',state,flush=True)
            except AssertionError as x: r='rejected: '+str(x); print('MOCK REJECT',x,flush=True)
        else: e={'code':-32601,'message':'no '+m}
        out=json.dumps({'result':r,'error':e,'id':q['id']}).encode()
        self.send_response(200);self.send_header('Content-Length',str(len(out)));self.end_headers();self.wfile.write(out)
HTTPServer(('127.0.0.1',19444),H).serve_forever()
