# Legal and naming

**Purpose.** The trademark, patent and design-right landscape with citations; what compatibility claims are permitted; the exact disclaimer text; the naming decision and the checks done; licence layout and attribution; the DCO; and toy-safety guidance for merchandise. This is planning input, not legal advice; a Maltese or EU trademark attorney should confirm the clearance items before launch.

## Contents

1. Jurisdiction
2. What is free and what is not
3. Permitted compatibility claims
4. Disclaimer text
5. Naming decision and checks
6. Licence layout and attribution
7. Contributor governance: DCO
8. Part designer guard rails
9. Merchandise compliance checklist
10. Customer artwork terms
11. Actions and open items

## 1. Jurisdiction

neuraldeep.net identifies the operator as Neural Deep Network LTD, based in Malta (company number C114166, Malta Business Registry; terms state Maltese law applies). OpenBrick3D is therefore an EU project: EU trade-mark law, the General Product Safety Regulation (GPSR, applies since 13 December 2024), the Toy Safety Directive 2009/48/EC today and Regulation (EU) 2025/2509 from 1 August 2030. The UK (CE still recognised in Great Britain under reg 39A) and the US (CPSIA, ASTM F963-23) are export markets. Confirm C114166 and VAT status on the registry (affects EU distance-selling obligations for merch).

## 2. What is free and what is not

| Item | Status | Source |
|------|--------|--------|
| Stud-and-tube brick coupling | Free. US patent 3,005,282 (1961) and related patents expired in 1978 (Canada 1988). | Google Patents; Wikipedia |
| Brick shape as a trademark | Not protectable: CJEU C-48/09 P, Lego Juris v OHIM, 14 Sept 2010 (shape necessary to obtain a technical result); Kirkbi v Ritvik, 2005 SCC 65; German BGH 2009. | curia.europa.eu |
| LEGO word mark and logo | Protected. Never in a product, app, package, domain, handle, icon, keyword, or embossed on studs. | EUIPO |
| Minifigure | Protected: valid EU 3D mark (General Court, Best-Lock 16 June 2015; T-297/22 BB Services, 6 Dec 2023) and US copyright (Lego v Zuru, D. Conn.; Fed. Cir. 2020; 2nd Cir. remand March 2025). Never generate minifigure bodies, heads or legs. | curia.europa.eu; court records |
| Newer element shapes | Registered Community designs last up to 25 years (Lego v Delta Sport, T-515/19 of 24 March 2021 and T-537/22 of 24 January 2024 upheld a brick design under the modular-systems exception). Exact copies of elements released within 25 years are design-right risk; generic stud-and-tube parts are not. | curia.europa.eu (verify dates) |
| LDraw Parts Library | CC BY 2.0 and CC BY 4.0 per file (new submissions since 23 Feb 2022 are 4.0; CC0 option since 2024). Commercial use permitted with attribution. LDraw is a trademark of the Estate of James Jessiman. | ldraw.org |
| LDCad shadow (connectivity) library | All rights reserved. Not redistributable. The SNAP syntax is a public spec and may be implemented. | melkert.net |
| LeoCAD, LDView, LPub3D, ImportLDraw | GPL. Never copy code into the MIT codebase; interoperate via files or separate processes. | project READMEs |
| Rebrickable data | Commercial use allowed with attribution; no scraping; no AI training. BrickLink catalog: restricted; do not ingest. | rebrickable.com ToS |

## 3. Permitted compatibility claims

"Compatible with LEGO® bricks" is referential use permitted under Art 14(1)(c) EUTMR, UK s.11(2)(c) TMA and US nominative fair use **if** it is an adjective, used once, in plain text, with the ® symbol, with a disclaimer, and without emphasis or the logo. LEGO's Fair Play guidance and the 2019 MyMiniFactory takedowns show that promotional emphasis crosses the line. We go further: product copy says "brick-compatible" or "compatible with leading stud-and-tube brick systems"; the LEGO word appears only in the disclaimer and in this document. While the ZURU litigation is live, we do not lead marketing with "LEGO compatible".

## 4. Disclaimer text

README, About screen, website footer, store listings:

> OpenBrick3D is an independent, community-driven open-source project published by Neural Deep Network Ltd (Malta). It is not affiliated with, sponsored by, authorized by, or endorsed by the LEGO Group. LEGO®, the LEGO® logo and the Minifigure are trademarks of the LEGO Group of companies, which does not sponsor, authorize or endorse this project. Works with LEGO®-compatible bricks. LDraw™ is a trademark of the Estate of James Jessiman. This software uses the LDraw Parts Library (https://www.ldraw.org).

Physical merchandise and product pages:

> Compatible with LEGO® bricks. Not a LEGO® product. LEGO® is a trademark of the LEGO Group, which does not sponsor, authorize or endorse this product.

## 5. Naming decision and checks

Checks run on 2026-10-27:

| Name or handle | Finding |
|----------------|---------|
| `cwesson/openBrick` on GitHub | exists: an OpenSCAD snap-block library, CC BY-NC-SA, commits 2017 to 2025 |
| "Open Bricks®" | live German brand: Open Brick Source GmbH & Co. KG, Drolshagen (HRA 9476 Siegen), custom-branded building-block sets, claims EUIPO registration; this is exactly the merchandise category |
| "OpenBrick" (openbrick.co) | a Madrid real-estate tokenisation fintech |
| openbrick.org / .com / .net / .app | taken (2008, 2018, 2008, 18 Oct 2025); GitHub user `openbrick` exists |
| OpenBrick3D | no hits; npm `openbrick3d` and `@openbrick` scope, crates.io `openbrick` and `open-brick`, PyPI `openbrick3d`, GitHub `openbrick3d`, domains `openbrick3d.com`, `openbrick3d.org`, `openbrick.dev`, `open-brick.org` all free |

**Decision (D23):** the open-source project and app are **OpenBrick3D** everywhere (the README already uses it); the CLI binary is `openbrick`; crates are `openbrick-*`. Register the free handles this week. Do **not** launch a commercial merchandise brand called OpenBrick, Open Brick or Open Bricks before a paid clearance search (EUIPO eSearch, TMview, UKIPO, USPTO, DPMA) in classes 9, 28, 35, 40 and 42; if the search confirms an EU class 28 mark for Open Bricks, give the merch line a distinct coined brand and file an EUTM (classes 9 and 42 now; 28 and 40 when merch starts; indicative fees EUR 850 for one class, plus 50 for a second, plus 150 each further) and a UK mark. Keep app-store names free of "LEGO". Publish `TRADEMARK.md`: the code is MIT, the name and logo are not.

## 6. Licence layout and attribution

| Tree | Licence | Notes |
|------|---------|-------|
| code (`crates/`, `packages/`, `apps/`) | MIT | existing LICENSE |
| `parts/`, `profiles/`, `standards/` | CC0 1.0 with a mandatory `credit` field as a social norm; CC BY 4.0 opt-in per file | contributors who want enforceable credit choose CC BY |
| `docs/` | CC BY 4.0 | |
| imported LDraw tree (catalog cache) | CC BY 2.0 / 4.0 per file, headers intact, separate from the repo | downloaded at first run; complete.zip is 140 MiB; crates.io caps crates at 10 MB |
| `fonts/` | SIL OFL 1.1, complete unmodified files with licence text | never subset bundled fonts |

Attribution obligations built into the product: every client has an attribution screen listing "This software uses the LDraw Parts Library" with a link, plus creator and licence of any derived part; `CREDITS.txt` is generated from the provenance blocks; converted binary caches are derivative works and carry the same credit. Ask the LDraw SteerCo for written confirmation that an aggregate credit satisfies attribution for a converted cache, and consider a donation or partnership.

## 7. Contributor governance: DCO

Developer Certificate of Origin 1.1 (`git commit -s`), enforced by the GitHub DCO app; an explicit inbound-equals-outbound clause in `CONTRIBUTING.md`; **no CLA** (MIT already permits every commercial use the company needs; a CLA would be justified only if a proprietary edition were planned). `LICENSE-POLICY.md` states the three-tier policy above.

## 8. Part designer guard rails

- Allow any stud-and-tube or anti-stud geometry.
- Block minifigure silhouettes and the LEGO logo in uploaded artwork (validator rule `TRADEMARK.LOGO`), with an explanation and no shaming.
- Never emboss text on stud tops.
- Show a design-right warning when a user reproduces a catalogued element first released within the last 25 years rather than a generic brick.
- Keep an audit trail (who uploaded what, when) for notice-and-takedown; register a DMCA agent (US) and implement DSA Article 16 notice-and-action (EU) if the company hosts uploads.

## 9. Merchandise compliance checklist

A product is a toy in the EU if play by under-14s is intended or reasonably foreseeable; a "14+" sticker alone does not exempt a logo tile or keychain. Decide per SKU:

**Route A: toy** (families, consumer keychains): EN 71-1, -2, -3 testing per material and colour family; CE marking and Declaration of Conformity; 10-year technical file; "Not suitable for children under 36 months" with the hazard text; manufacturer name, Malta address, email and batch id on product or packaging; GPSR distance-selling information. US: ASTM F963-23 (mandatory for toys made after 20 April 2024), CPSC-accepted lab testing, Children's Product Certificate and tracking label for 12-and-under, 16 CFR 1501 small-parts rules (test cylinder 31.7 x 57.1 mm). Consider CPSC small-batch manufacturer registration.

**Route B: adult promotional goods** (B2B corporate orders): explicit "not a toy, not for under-14s" marking; GPSR still applies (responsible operator, listing information, Safety Business Gateway registration, recalls).

Both routes: build the EU Digital Product Passport fields (unique product id, economic-operator id, CE, warnings, QR data carrier) into the product and order model now; mandatory from 1 August 2030. For Great Britain rely on CE under reg 39A and name a GB importer or fulfilment partner. Prefer filament vendors with EN 71-3 and CPSIA test reports to reduce per-SKU lab cost; for children's-grade output consider SLS/MJF bureaus with existing reports.

## 10. Customer artwork terms

Customer warrants ownership or licence of uploaded artwork and indemnifies; the company may refuse or cancel; automatic refusal of LEGO marks and minifigures plus a curated blocklist of well-known marks; notice-and-takedown contact; retention of upload records for the limitation period; GDPR notice for uploaded artwork; print-ready files are the customer's property, with a licence to produce; anonymised previews shown only with consent.

## 11. Actions and open items

1. Register the free handles and domains (this week).
2. Commission the paid clearance search for OpenBrick3D and for the merch brand candidates.
3. Confirm C114166 and VAT registration on the Malta Business Registry.
4. Publish `TRADEMARK.md`, `LICENSE-POLICY.md`, `CONTRIBUTING.md` with DCO, `CODE_OF_CONDUCT.md` (M0).
5. Ask Roland Melkert about LDCad shadow data; ask the LDraw SteerCo about aggregate credit for caches.
6. Track BB Services (minifigure) and Lego v Zuru outcomes; a loss for LEGO would widen what the designer may generate.
7. Decide whether the company hosts community uploads (DSA obligations) or only mirrors a Git registry.
