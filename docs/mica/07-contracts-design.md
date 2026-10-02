# 07 — Contract-Design: YeetPayments & PaperWalletEscrow

**Status: ENTWURF** · Solidity 0.8.24, OpenZeppelin v5.1.0, Foundry.
Quellen: `contracts/src/YeetPayments.sol`, `contracts/src/PaperWalletEscrow.sol`,
Tests: `contracts/test/*.t.sol` — **`forge test`: 40/40 grün** (Stand 2026-09-30,
Forge 1.8.4). **Externes Audit vor Deploy erforderlich (F8).**

Diese Contracts setzen die Funktionsanpassungen **F1/F2** (Zahlungen strikt
Wallet↔Wallet) und **F3** (Paper Wallets als On-Chain-Escrow) aus
[06-leitplanken-validierung.md](06-leitplanken-validierung.md) technisch um.

---

## 1. YeetPayments — Tips, PPV, Promotion (non-custodial)

### 1.1 Prinzip
Eine einzige `pay(kind, recipient, ref, amount)`-Funktion zieht YEET per
`transferFrom` **direkt** aus der Wallet des Zahlers und teilt atomar:
`fee → platformWallet`, `net → recipient`. Der Contract **hält nie** Guthaben.
`kind` (Tip/PayPerView/Promotion) ist nur ein Routing-Hinweis fürs Event.

### 1.2 Guardrail-Mapping
| Guardrail | Umsetzung |
| --- | --- |
| L2 (nie interne YEET-Gutschrift) | Contract hat keine Guthaben-Logik; `recipient` bekommt Token direkt. Ist der Creator ohne Wallet, ruft das Frontend die Funktion **gar nicht** auf → Zahlung in Punkten oder Feature aus (Off-Chain-Enforcement) |
| Kein Custody | `balanceOf(YeetPayments) == 0` als Invariante (Test `test_ContractNeverHoldsFunds`) |
| Fee unschädlich | `platformFeeBps` ≤ 20 %, Config-only, bewegt keine Fremdmittel |
| Pausable ok | `whenNotPaused` blockt nur NEUE Zahlungen; da nichts escrowed ist, kann Pause keine Mittel einsperren |

### 1.3 Wichtige Design-Entscheidungen
- **Ownable2Step** statt `Ownable`: Zwei-Schritt-Ownership-Übergabe verhindert
  versehentlichen Verlust der Admin-Rolle (Best Practice; Emittentenpflicht
  „ordentliche Governance").
- **`_pay` intern:** Ein einziger Code-Pfad; nie über `this.pay(...)` extern
  aufrufen (das würde `msg.sender` auf den Contract umschreiben und aus der
  falschen Wallet ziehen — beim Design gefunden und behoben).
- **`ref` als bytes32:** Off-Chain-UUID (Post/Content/Promotion) → `bytes32`.
  Der Indexer verknüpft On-Chain-Events mit DB-Objekten.
- **Kein „amountReceived"-Vertrauen:** Bei Fee-on-Transfer-Token wäre der Split
  unsauber — YEET ist ein Standard-ERC-20 ohne Transfer-Fee, daher unkritisch;
  im Audit bestätigen.

## 2. PaperWalletEscrow — Bearer-Gutscheine (non-custodial Escrow)

### 2.1 Prinzip
Der Aussteller sperrt **eigene** YEET in einen Voucher, der über die
**Adresse eines Ephemeral-Keypairs** identifiziert wird. Der private Schlüssel
dieses Keypairs steckt im Paper-Wallet-QR. Wer ihn hat, kann einlösen; nach
Ablauf ist der Betrag an den Aussteller rückerstattbar.

```
create(claimAddr, amount, expiry)   // Aussteller lockt eigene Token
claim(claimAddr, recipient, sig)    // Einlöser zahlt an recipient
refund(claimAddr)                   // nach expiry, immer an Aussteller
```

### 2.2 Das Front-Running-Problem und die Lösung
Bei einem **naiven Hash-Lock** (`claim(secret)` zahlt an `msg.sender`) sieht ein
Mempool-Bot das `secret`, kopiert es und claimt in **seine** Adresse, bevor die
Original-Transaktion durchgeht → Diebstahl.

**Lösung hier:** Der Einlöser **signiert die Empfängeradresse** mit dem
Ephemeral-Key. Der Contract rekonstruiert den Signierer und verlangt
`signer == claimAddr`. Die Signatur ist domain-gebunden (Voucher, Empfänger,
`chainId`, Contract-Adresse). Ein Front-Runner kann den Empfänger nicht ändern,
ohne die Signatur ungültig zu machen, und ohne den privaten Schlüssel keine
neue Signatur erzeugen. → Test `test_FrontRunWithSwappedRecipientReverts`.

### 2.3 Guardrail-Mapping (L5+)
| Guardrail | Umsetzung |
| --- | --- |
| Nicht upgradeable | Plain Contract, kein Proxy, kein `delegatecall` |
| Kein Admin-Sweep | **Keine** `rescue`/`withdraw`-Funktion; Owner kann Escrow nie bewegen (Test `test_NoAdminSweep`) |
| claim/refund immer möglich | `issuancePaused` blockt **nur** `create`; claim/refund sind nie pausierbar (Test `test_IssuancePauseBlocksCreateNotClaim`) |
| Refund nur an Aussteller | `refund` zahlt immer an gespeicherten `issuer`, callable by anyone (falls Schlüssel verloren) |
| Betragslimits (F3/AML) | `maxVoucherAmount`, `min/maxValidity` — Config-only, berühren kein Escrow |
| Front-Running | Signatur bindet Empfänger (§2.2) |
| Reentrancy | `nonReentrant` + Effects-before-Interactions + `SafeERC20` |

### 2.4 Offene Design-Entscheidungen
- **Fee auf Gutscheine:** Standard `platformFeeBps = 0` (Gutscheine sind
  Geschenke). TODO(strategie): Fee gewünscht? (Cap 10 %.)
- **Betragsobergrenze:** `maxVoucherAmount` beim Deploy setzen.
  TODO(strategie): Gegenwert (z. B. ~150 €) — bewusst niedrig für AML-Optik.
- **Speicher-Reuse:** Voucher werden nach Redemption nicht gelöscht (Reentrancy-
  Sicherheit + Historie). `claimAddr` ist einmalig, daher kein Reuse nötig.

## 3. Client-Flow (Frontend)

### 3.1 Wallet-Anbindung
- Injected Provider (MetaMask) + **WalletConnect v2** (Trust Wallet u. a.).
- Zahlungen: erst `token.approve(YeetPayments, amount)` (oder `permit` via
  `ERC20Permit` — YeetToken unterstützt es → 1 Transaktion spart), dann
  `pay(...)`.

### 3.2 Paper Wallet ausstellen
1. Client erzeugt ein **Ephemeral-Keypair** (z. B. `ethers.Wallet.createRandom()`).
2. `approve` + `create(wallet.address, amount, expiry)` aus der Aussteller-Wallet.
3. QR enthält den **privaten Schlüssel** (bzw. Mnemonic) des Ephemeral-Keypairs
   — wie beim bisherigen Claim-Secret, nur ist es jetzt ein echter EC-Key.
   ⚠️ Der Server sieht diesen Schlüssel **nicht** (Non-Custody bleibt gewahrt).

### 3.3 Paper Wallet einlösen
1. Empfänger scannt QR → Client hat den Ephemeral-Key.
2. Client baut `escrow.claimDigest(claimAddr, meineWallet)`, signiert ihn mit
   dem Ephemeral-Key, ruft `claim(claimAddr, meineWallet, sig)` — die
   Transaktion kann aus einer beliebigen Wallet (auch einem Relayer) gesendet
   werden; der Empfänger ist per Signatur fixiert.
3. E-Mail-only-Nutzer: müssen zuvor eine Auszahl-Wallet verknüpfen (Doc 05);
   danach ist `recipient` ihre verifizierte Adresse.

## 4. Backend-Integration (Indexer)

- **Ersetzt** die Off-Chain-Buchungen in `tips.rs` / PPV-`unlock` / `paper_wallets.rs`
  durch **Event-Indexing**:
  - `YeetPayments.Paid(kind, payer, recipient, ref, gross, fee, net)` →
    Zähler aktualisieren; bei `kind==PayPerView` PPV für `ref` freischalten.
  - `PaperWalletEscrow.VoucherClaimed/Refunded` → Gutschein-Status.
- **Bestätigungen abwarten** (BSC: z. B. 15 Blocks) bevor Freischaltung.
- **Kein Server-Key mit Zugriff auf Nutzer-/Escrow-Mittel.** Der bestehende
  Reward-Minter-Key (`batchMintRewards`) bleibt getrennt und mintet nur aus der
  75 %-Reward-Reserve (`REWARD_RESERVE`, 15,75 Mrd. — on-chain gedeckelt über
  `rewardsMinted`); eine generische `mint()`-Funktion gibt es nicht mehr. Multisig
  empfohlen (F8; Token ist `Ownable2Step`).
- Migration: Alt-Guthaben-Ledger einfrieren; siehe Doc 05 §4.5.

## 5. Deploy-Reihenfolge

1. `YeetToken` (existiert) — Adresse fix.
2. `YeetPayments(token, platformWallet, owner)`.
3. `PaperWalletEscrow(token, platformWallet, owner, maxVoucherAmount, minValidity, maxValidity)`.
4. Owner → Multisig übertragen (`transferOwnership` + `acceptOwnership`, Ownable2Step).
5. `Deploy.s.sol` um beide erweitern. TODO(dev).
6. Verifizieren (bscscan), Adressen in Backend-Env + Frontend eintragen.

## 6a. Stand der lokalen Prüfung (2026-09-30)

- `forge build` + `forge test`: alle fünf Contracts kompilieren, 40/40 Tests grün
  (Token 10, NFT 6, Tipping 6, Payments 8 inkl. Fuzz `testFuzz_SplitConserves`,
  Escrow 10). Drei Escrow-Tests waren zuvor fehlerhaft formuliert
  (`vm.expectRevert` band sich an den vorgelagerten `claimDigest()`-View-Call
  statt an `claim()`; beim No-Admin-Sweep-Test liefert ein Low-Level-Call
  unter `expectRevert` `ok = true`) — die Contracts selbst verhielten sich
  korrekt.
- Toolchain-Hinweis: OpenZeppelin ≥ 5.2 nutzt `mcopy` (Cancun); mit
  solc 0.8.24 ohne `evm_version = "cancun"` bricht der Build. Pin: OZ 5.1.0
  (`forge install OpenZeppelin/openzeppelin-contracts@v5.1.0`), Remappings in
  `contracts/remappings.txt`.
- `forge lint` (Forge 1.8.4), als Input fürs Audit — keiner der Befunde ist ein
  bekannter Fehler, alle sind bewusst oder durch Guards abgedeckt:
  | Regel | Wo | Einordnung |
  | --- | --- | --- |
  | `block-timestamp` (5×) | PaperWalletEscrow `create`/`claim`/`refund`/`claimDigest` | Ablauflogik braucht die Blockzeit; Miner-Spielraum (Sekunden) ist gegenüber Tages-Gültigkeiten unerheblich |
  | `unsafe-typecast` (1×) | PaperWalletEscrow `uint96(amount)` | durch `AmountOverflow`-Check davor abgesichert |
  | `missing-events-arithmetic` (3×) | Escrow `setLimits`, Tipping `setPlatformFee`, Payments Fee-Setter | Events vorhanden bzw. Config-only; für Off-Chain-Monitoring beim Audit prüfen |
  | `require-revert-in-loop` (1×) | YeetToken `batchMintRewards` | ein ungültiger Empfänger revertet den ganzen Batch — gewollt (Batch ist atomar; Backend validiert vorab, F6-Screening) |
  | `reentrancy-events` (1×) | YeetNFT `mintPost` (Event nach `_safeMint`-Callback) | nur Log-Reihenfolge; keine Zustandsänderung nach dem Callback |

## 6. Sicherheits-Checkliste vor Mainnet (F8)

- [ ] Externes Audit beider Contracts
- [ ] Slither/Mythril clean
- [x] Fuzz-/Invariant-Tests (Contract hält nie Fremdmittel außer aktivem Escrow) — `testFuzz_SplitConserves`, `test_ContractNeverHoldsFunds`, `test_NoAdminSweep` grün
- [ ] Reentrancy-Review (SafeERC20 + Guards vorhanden)
- [x] Front-Running-Review Paper Wallet (Signatur-Bindung) — `test_FrontRunWithSwappedRecipientReverts`, `test_AttackerWithoutKeyCannotForge` grün; externe Bestätigung im Audit
- [ ] Ownership auf Multisig, Owner-Funktionen minimiert
- [ ] Kein Proxy / kein `delegatecall` / kein `selfdestruct`
- [ ] YEET ist kein Fee-on-Transfer/rebasing Token (Split-Annahme bestätigen)
