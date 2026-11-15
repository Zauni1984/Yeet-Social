# 11 — Nachhaltigkeitsindikatoren (Art. 6 Abs. 1 lit. j MiCA, ESMA-RTS)

Stand: 2026-11-13 · Status: **Methodik und Rechner fertig, Werte noch zu erheben** (alle Zahlen in
`tools/bsc-inputs.json` sind mit `TODO(verify)` markiert und nur Größenordnungen).

## 1. Was offengelegt werden muss

Die ESMA-RTS zu Art. 6(1)(j) (Delegierte Verordnung (EU) 2025/422, Anhang) verlangt für den
Konsensmechanismus der genutzten DLT:

| Nr. | Indikator | Einheit | Wann |
| --- | --- | --- | --- |
| 1 | Energieverbrauch des Netzes pro Jahr | kWh | immer |
| 2 | Anteil erneuerbarer Energien am Verbrauch | % | wenn Nr. 1 > 500.000 kWh |
| 3 | Energieintensität | kWh je validierte Transaktion | wenn Nr. 1 > 500.000 kWh |
| 4 | THG-Emissionen Scope 1 (direkt kontrolliert) | tCO2e p. a. | wenn Nr. 1 > 500.000 kWh |
| 5 | THG-Emissionen Scope 2 (eingekaufte Energie) | tCO2e p. a. | wenn Nr. 1 > 500.000 kWh |
| 6 | THG-Intensität | kgCO2e je validierte Transaktion | wenn Nr. 1 > 500.000 kWh |
| 7 | Qualitative Angaben: Konsensmechanismus, Anreize, Datenquellen, Methodik, Zeitraum | Text | immer |

Freiwillig (ESMA „optional indicators“): Abfallaufkommen (E-Schrott), Nutzung natürlicher
Ressourcen (Wasser), Anteil an gefährlichen Abfällen. Für BSC liegen dafür keine belastbaren
Netzdaten vor; das Whitepaper sagt das so.

## 2. Methodik (CCRI-Bottom-up für Proof-of-Staked-Authority)

BNB Smart Chain ist PoSA mit einer festen, kleinen Validatormenge (zum Stichtag erheben) plus
Kandidaten. Der Verbrauch wird je Knoten geschätzt:

```
kWh/Jahr        = Knoten × (W je Knoten × PUE) × 8.760 h / 1.000
kWh/Transaktion = kWh/Jahr ÷ Transaktionen/Jahr
Scope 2 (t)     = kWh/Jahr × Σ(Standortanteil × Netzfaktor kgCO2e/kWh) / 1.000
Erneuerbaren-%  = Σ(Standortanteil × Erneuerbarenanteil des Standorts)
```

- **Knoten:** aktive Validatoren + Kandidaten (BSC-Staking-Dashboard); öffentliche Full Nodes
  optional, sofern eine belastbare Zahl existiert.
- **Leistung je Knoten:** gemessenes Profil eines Servers nach den BNB-Chain-Validatoranforderungen
  (16 Kerne, 64 GB RAM, NVMe) oder der CCRI-Referenzwert; PUE als Rechenzentrumsdurchschnitt.
- **Standorte:** Verteilung nach Validator-Hosting (EU / Nordamerika / Asien), Netzfaktoren EEA,
  EPA eGRID, IEA.
- **Scope 1:** 0, sofern keine eigenen Aggregate; sonst Diesel-Notstrom.
- **Zuordnung:** Yeet rechnet sich den Anteil der selbst verursachten Transaktionen zu
  (Mint-Batches, Swaps); die Netzwerte werden daneben vollständig genannt.

Alternative mit geringerem Aufwand: die CCRI-API (`v2.api.carbon-ratings.com`, Endpunkte
`currencies/bnb/electricity-consumption/network` und `…/emissions/network`) liefert die
Netzwerte direkt — benötigt einen kostenpflichtigen API-Key; die Werte sind ESMA-kompatibel
aufbereitet. Beides ist zulässig, die Quelle wird im Whitepaper benannt.

## 3. Rechner

```
python3 docs/mica/tools/sustainability.py docs/mica/tools/bsc-inputs.json
```

Liest die Netzfakten aus der JSON-Datei (jede Zahl mit Quellenzeile), prüft die 500-MWh-Schwelle
und gibt die Tabelle für Whitepaper Teil I als Markdown aus. Mit Platzhaltern ergibt sich derzeit
ein Wert unterhalb der Schwelle — **das ist keine Aussage**, sondern folgt aus den
Platzhalter-Eingaben; mit erhobenen Werten kann die Schwelle überschritten sein, dann sind die
Indikatoren 2–6 pflichtig (der Rechner gibt sie ohnehin aus).

## 4. Erhebung (To-do, Betreiber/Dev, ca. 2 h)

1. Stichtag festlegen (z. B. letzter Tag des Vorquartals) und 12-Monats-Zeitraum.
2. Validator- und Kandidatenzahl von bscscan.com/validators, Transaktionen/Tag aus dem
   bscscan-Chart (12-Monats-Mittel) eintragen.
3. Leistungsprofil: CCRI-Referenz oder Messung; PUE aus der aktuellen Uptime-Institute-Umfrage.
4. Standortverteilung: Hosting-Regionen der Validatoren (Staking-Dashboard/Explorer) → Anteile,
   Netzfaktoren und Erneuerbarenanteile je Region mit Quelle und Jahr.
5. Yeet-Anteil: eigene Transaktionszahl (Mint-Batches, Swaps) ÷ Netztransaktionen.
6. Rechner laufen lassen, Tabelle und Quellen in Whitepaper Teil I übernehmen, `TODO(verify)`
   entfernen; iXBRL-Fassung durch den Dienstleister (Checkliste).
7. Jährlich wiederholen; bei Chain-Wechsel neu erheben (Chain-Assessment, Dokument 03).
