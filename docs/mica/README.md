# MiCA-Compliance-Dokumentation — Yeet Social / YEET Token

**Status: ENTWURF — laufend anpassbar, bis die finale Tokenisierungsstrategie feststeht.**

> ⚠️ **Kein Rechtsrat.** Diese Dokumente sind eine strukturierte Arbeitsgrundlage auf Basis
> der Verordnung (EU) 2023/1114 (MiCA / MiCAR). Vor einem öffentlichen Angebot, einem
> Listing oder dem EU-Livegang der Token-Funktionen muss eine auf Kryptorecht
> spezialisierte Kanzlei die Einstufung und die Dokumente prüfen.

## Inhalt

| Datei | Zweck | Status |
| --- | --- | --- |
| [01-readiness-assessment.md](01-readiness-assessment.md) | Einstufung des YEET-Tokens, anwendbare Pflichten, Ausnahmen, CASP-Risikoanalyse | Entwurf |
| [02-whitepaper-entwurf-yeet.md](02-whitepaper-entwurf-yeet.md) | Krypto-Whitepaper-Gerüst nach MiCA Titel II (Art. 6 + Anhang I) mit Platzhaltern | Entwurf |
| [03-chain-assessment.md](03-chain-assessment.md) | BNB Chain unter MiCA + Alternativen-Matrix + Nachhaltigkeitsangaben | Entwurf |
| [04-compliance-checkliste.md](04-compliance-checkliste.md) | Abarbeitbare Checkliste mit Verantwortlichkeiten | Entwurf |
| [05-zielarchitektur-non-custodial.md](05-zielarchitektur-non-custodial.md) | Zielarchitektur „Punkte + Direktauszahlung" (non-custodial) inkl. MiCA-Bewertung und Leitplanken | Entwurf |
| [06-leitplanken-validierung.md](06-leitplanken-validierung.md) | Adversariale Prüfung der Leitplanken L1–L7 (Aufsichts-/Anwaltsperspektive) + Nachbarregime + Funktionsanpassungen F1–F8 | Entwurf |

## Wie diese Dokumente gepflegt werden

- Alle offenen Entscheidungen sind mit `TODO(strategie):` markiert — sie hängen von der
  finalen Tokenisierungsstrategie ab (Verkauf ja/nein, Listing ja/nein, Ledger-Modell).
- Alle zu verifizierenden Fakten sind mit `TODO(verify):` markiert.
- Änderungen bitte per PR, damit die Historie der Compliance-Annahmen nachvollziehbar bleibt.

## Kernaussagen (Stand dieses Entwurfs)

1. **YEET ist ein Krypto-Wert nach MiCA** (weder ART noch EMT) → Titel II ist der Maßstab.
2. **Solange kein Verkauf und kein Listing erfolgt, ist ein Whitepaper voraussichtlich noch
   nicht pflichtig** — aber die "Gratis"-Ausnahme ist wegen Registrierungsdaten/Content als
   Gegenleistung unsicher; Details in Dokument 01.
3. **Das größte Risiko ist nicht der Token, sondern der interne Guthaben-Ledger**
   (Tips/PPV über `users.yeet_token_balance`): Das kann als Verwahrung und Verwaltung
   bzw. Transferdienstleistung für Kunden gelten → CASP-Zulassungspflicht. Details in 01, §4.
4. **"MiCA-Konformität" ist keine Eigenschaft einer Blockchain.** MiCA verpflichtet
   Personen (Anbieter, Emittenten, CASPs), nicht Netzwerke. BNB Chain ist daher nicht
   "nicht MiCA-konform" — relevant sind Offenlegungspflichten (u. a. Nachhaltigkeits-
   indikatoren des Konsensmechanismus) und praktische Kriterien. Details in 03.
