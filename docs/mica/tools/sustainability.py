#!/usr/bin/env python3
"""ESMA/MiCA sustainability indicators for a Proof-of-Staked-Authority chain.

Computes the mandatory and supplementary indicators of the ESMA RTS on
sustainability disclosures (Art. 6(1)(j) MiCA) from a small JSON input of
network facts, using the bottom-up method that CCRI applies to PoS/PoSA
networks: validator hardware power × PUE × hours, split by location to apply
grid emission factors. Inputs stay in the JSON so every figure in the
whitepaper has a source line next to it.

    python3 docs/mica/tools/sustainability.py docs/mica/tools/bsc-inputs.json

Prints a Markdown table for whitepaper Part I and states whether the
500 000 kWh/year threshold triggers the supplementary indicators.
"""
import json, sys

def main(path):
    d = json.load(open(path))
    nodes = d["validators_active"] + d["validators_standby"] + d.get("full_nodes_public", 0)
    watts = d["watts_per_node"] * d["pue"]
    kwh_year = nodes * watts * 24 * 365 / 1000.0
    tx_year = d["tx_per_day"] * 365
    kwh_per_tx = kwh_year / tx_year if tx_year else float("nan")
    # Location-weighted grid factor (kgCO2e/kWh); shares must sum to 1.
    gf = sum(l["share"] * l["kg_co2e_per_kwh"] for l in d["locations"])
    ren = sum(l["share"] * l["renewable_share"] for l in d["locations"])
    scope2_t = kwh_year * gf / 1000.0
    scope1_t = d.get("scope1_t_co2e", 0.0)
    kg_per_tx = (scope1_t + scope2_t) * 1000 / tx_year if tx_year else float("nan")
    above = kwh_year > 500_000
    rows = [
        ("Energieverbrauch p. a. (kWh)", f"{kwh_year:,.0f}", "Pflicht"),
        ("Anteil erneuerbarer Energien (%)", f"{ren*100:.1f}", "ab 500 MWh/a"),
        ("Energieintensität (kWh je Transaktion)", f"{kwh_per_tx:.6f}", "ab 500 MWh/a"),
        ("THG Scope 1 (tCO2e p. a.)", f"{scope1_t:,.2f}", "ab 500 MWh/a"),
        ("THG Scope 2 (tCO2e p. a.)", f"{scope2_t:,.2f}", "ab 500 MWh/a"),
        ("THG-Intensität (kgCO2e je Transaktion)", f"{kg_per_tx:.6f}", "ab 500 MWh/a"),
        ("Anteil des Projekts am Netz (%)", f"{d['project_share_of_network']*100:.4f}", "Zuordnung"),
        ("Energieverbrauch, dem Projekt zugerechnet (kWh p. a.)", f"{kwh_year*d['project_share_of_network']:,.1f}", "Zuordnung"),
    ]
    print(f"Netzknoten: {nodes} · Leistung je Knoten inkl. PUE: {watts:.0f} W · Tx/Jahr: {tx_year:,.0f}")
    print(f"Schwelle 500.000 kWh/Jahr {'ÜBERSCHRITTEN → ergänzende Indikatoren pflichtig' if above else 'nicht überschritten'}\n")
    print("| Indikator | Wert | Pflicht/Zusatz |")
    print("| --- | --- | --- |")
    for r in rows:
        print(f"| {r[0]} | {r[1]} | {r[2]} |")
    print("\nQuellen (aus der Eingabedatei):")
    for k, v in d.get("sources", {}).items():
        print(f"- {k}: {v}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "docs/mica/tools/bsc-inputs.json")
