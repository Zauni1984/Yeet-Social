# WalletConnect v2 (Reown)

Neben MetaMask & Co. (injizierter Provider) kann man sich per **WalletConnect v2**
mit einer mobilen Wallet anmelden (Trust Wallet, MetaMask Mobile, Rainbow, …):
QR-Code am Desktop, Deep Link am Smartphone.

## Aktivieren

1. Kostenloses Projekt unter <https://cloud.reown.com> anlegen (Name „YEET Social“,
   Domain `justyeet.it`), die **Project ID** kopieren.
2. Auf dem VPS in `/root/yeet-social/.env`:
   ```
   WALLETCONNECT_PROJECT_ID=<deine Project ID>
   ```
   dann Backend neu starten. Die ID ist nicht geheim (sie landet ohnehin im
   Browser); das Frontend holt sie über `GET /api/v1/config`.
3. Ohne ID bleibt der WalletConnect-Button im Login-Dialog bei „kommt bald“.

## Wie es funktioniert

- `window.YeetWallet` (am Ende von `frontend/index.html`) ist der Provider-Broker:
  `getProvider()` liefert die WalletConnect-Sitzung, wenn der Nutzer sich so
  angemeldet hat (wird aus dem Speicher der Bibliothek wiederhergestellt, QR nur
  bei `interactive`), sonst `window.ethereum`. Alle Signatur-Stellen
  (Login, Wallet verknüpfen, E2EE-Identität, Chain-Wechsel) nutzen ihn.
- Die Bibliothek (`@walletconnect/ethereum-provider@2.25.0`, ~1,4 MB) wird erst
  beim ersten Klick auf „WalletConnect“ als ES-Modul von `cdn.jsdelivr.net`
  geladen (`+esm`-Bundle); wer E-Mail oder MetaMask nutzt, lädt nichts davon.
  Pin der Version im Modul (`WC_ESM`).
- Login: `connectWalletConnectAuth()` → `provider.connect()` (Modal) →
  derselbe Nonce/Signatur/Verify-Ablauf wie bei MetaMask (`_walletAuth`).
  `localStorage.yeet_wallet_kind = 'walletconnect'` merkt sich den Weg;
  Logout beendet die WalletConnect-Sitzung.
- Chain: `chains: [56]` (BSC Mainnet aus `window.YEET_CHAIN`), `rpcMap` auf den
  konfigurierten RPC.

## Datenschutz

Nur bei Nutzung von WalletConnect verbindet sich der Browser mit dem Relay
`relay.walletconnect.com` (WalletConnect Foundation / Reown). Das steht in der
Datenschutzerklärung (EN + DE, „Inhalte Dritter“).

## Weiteres

`GET /api/v1/config` liefert außerdem `yeet_token_address`
(`YEET_TOKEN_ADDRESS`) und `chain_id`; das Frontend übernimmt die Token-Adresse
in `window.YEET_CHAIN.token`, sobald sie auf dem Server gesetzt ist — der
`TODO(dev)`-Platzhalter in `index.html` muss dafür nicht mehr editiert werden.
