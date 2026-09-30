# Sanktions-Screening der Auszahlungsadressen (F6)

Umsetzung von **F6** aus der MiCA-Compliance-Checkliste
([docs/mica/04](mica/04-compliance-checkliste.md), Begründung in
[06 §8](mica/06-leitplanken-validierung.md)): Bevor YEET on-chain an eine
Adresse geht, wird die Adresse gegen eine Sanktionsliste geprüft.

## Was geprüft wird

| Stelle | Verhalten bei Treffer |
| --- | --- |
| Wallet verknüpfen (`POST /api/v1/auth/link-wallet/verify`) | `403 FORBIDDEN / SANCTIONED_ADDRESS` – die Adresse wird nicht verknüpft |
| Punkte → YEET (`POST /api/v1/points/convert`) | `403 FORBIDDEN / SANCTIONED_ADDRESS` – keine Punkte werden abgebucht, kein Payout entsteht |
| Batch-Mint (stündlicher Job `batch_rewards`) | Die Zeile wird als `failed` mit `last_error = SANCTIONED_ADDRESS …` geparkt und erscheint in der Admin-Payout-Queue (Ablehnen → Punkte zurück). Die übrigen Payouts des Batches gehen normal raus. |

Das Frontend zeigt bei `SANCTIONED_ADDRESS` den Text `convert.sanctioned`
(in allen UI-Sprachen).

## Fail-closed

Solange seit dem Backend-Start noch **keine** Liste geladen werden konnte,
werden Batch-Payouts **zurückgehalten** (nicht als failed markiert) und beim
nächsten Lauf erneut versucht; der Loader probiert es alle 5 Minuten. Ist
einmal eine Liste geladen, bleibt bei einem fehlgeschlagenen Refresh die
letzte gute Liste aktiv. Beim Verknüpfen/Konvertieren ohne geladene Liste
wird nicht blockiert – der Mint-Job ist die harte Schranke.

## Quelle

Standard ist die Liste der **OFAC-SDN-Kryptoadressen für Ethereum-kompatible
Chains** im Klartextformat (ein Address je Zeile) aus dem öffentlichen
Spiegel `0xB10C/ofac-sanctioned-digital-currency-addresses`, der aus der
offiziellen SDN-Liste des US-Finanzministeriums generiert wird. BNB Smart
Chain teilt den EVM-Adressraum, die ETH-Liste gilt daher 1:1.

> Hinweis: Die EU-Sanktionslisten (z. B. Anhang I der VO 269/2014) enthalten
> derzeit keine Krypto-Adressen in maschinenlesbarer Form. Wer zusätzliche
> Adressen (EU/UK/intern) prüfen will, hängt sie über `SANCTIONS_LIST_FILE`
> oder `SANCTIONS_EXTRA_ADDRESSES` an.

## Konfiguration

| Variable | Default | Bedeutung |
| --- | --- | --- |
| `SANCTIONS_SCREENING` | `on` | `off` schaltet die Prüfung komplett ab (wird beim Start laut geloggt) |
| `SANCTIONS_LIST_URL` | OFAC-ETH-Liste (0xB10C-Spiegel) | Newline-Liste; `none` = keine Remote-Liste |
| `SANCTIONS_LIST_FILE` | – | lokale Datei, wird zusätzlich eingelesen |
| `SANCTIONS_EXTRA_ADDRESSES` | – | kommagetrennte Adressen, zusätzlich |
| `SANCTIONS_REFRESH_HOURS` | `24` | Aktualisierungsintervall |

Zeilen mit `#` und leere Zeilen werden ignoriert; bei CSV-Zeilen zählt die
erste Spalte. Adressen werden kleingeschrieben verglichen.

## Betrieb

- Log beim Start: `sanctions: list loaded (N addresses)`.
- Treffer im Log: `batch-rewards: reward … payout address … is on the sanctions list`.
- Ein geparkter Payout ist im Admin-Dashboard (Payout-Queue, Filter „failed")
  am roten `last_error` erkennbar.
