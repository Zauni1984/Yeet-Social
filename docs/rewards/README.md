# Public rewards guide

`rewards-de.html` and `rewards-en.html` are the sources of the public PDF
"Das YEET-Belohnungssystem" / "The YEET Rewards System", linked from the site
footer. Both share `rewards.css`.

Build (headless Chromium via Playwright):

```
node scripts/build-rewards-pdf.cjs
```

writes `frontend/yeet-rewards-de.pdf` and `frontend/yeet-rewards-en.pdf`.
The PDFs are committed; `vps/cd-deploy.sh` copies them into the web root and
`nginx.conf` serves them from a dedicated location.

The guide must match the code. When a rule changes, update in this order:

1. the code (`backend/src/services/tokens.rs`, `api/tips.rs`, `api/points.rs`, …),
2. the Terms of Service in `frontend/index.html` if the change is material,
3. both HTML sources (bump version and date, add a line to the change history),
4. rebuild the PDFs.

The numbers in force are also served by `GET /api/v1/tokens/params`.
