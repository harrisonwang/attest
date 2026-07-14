# P1 cold-start validation

> Accepted 2026-07-14 from five public repositories absent from the 280-case corpus and the top-50 launch scan. Repository snapshots were frozen and scanned before inspecting any finding; the engine was not changed after this accepted cohort was selected.

## Accepted cohort

| Repository | Snapshot | Docs | Broken reviewed | False positives | Result |
|---|---|---:|---:|---:|---|
| `nextdns/nextdns` | `cd213c985a44` | 1 | 0 | 0 | clean cold start |
| `sei-protocol/sei-chain` | `959d1275600b` | 7 | 0 | 0 | clean cold start |
| `gap-system/gap` | `afe80eeb2aaf` | 1 | 0 | 0 | clean cold start |
| `woocommerce/woocommerce-gateway-stripe` | `87a684d18333` | 14 | 9 | 0 | stale test directories and six moved/renamed agentic-commerce test files confirmed |
| `1Password/SCAM` | `ce2761e85301` | 1 | 0 | 0 | clean cold start |
| **Total** |  | **24** | **9** | **0** | **P1 acceptance satisfied** |

All nine WooCommerce findings were checked against the snapshot tree. `Admin/`, `PaymentMethods/`, and `PaymentTokens/` now use lowercase kebab-case directories; the six named `WC_Stripe_Agentic_Commerce_*_Test.php` files now live under `tests/phpunit/agentic-commerce/` with WordPress-style kebab-case filenames.

The complete report, commit SHAs, finding contexts, and resolver evidence are stored in `reports/cold-start-validation.json` (`attest.scan.v1`): 5/5 successful repositories, 24 documents, 920 tokens, 9 broken, 35 suspect, and 0 reviewed false-positive broken findings.

## Rejection audit

Earlier candidate cohorts are not counted. The 2026-07-12 evidence incorrectly treated Open Pencil’s explicitly external `figma-use` paths as local drift. A 2026-07-14 hardening pilot then exposed external SDK shorthand in tldraw and a date-template placeholder in Struts; those cases drove context-guard fixes and were excluded. `diffblue/cbmc` was also rejected because its output paths are generated and `make -C unit test` is valid. These rejections prevent post-hoc tuning from being mislabeled as cold-start precision.
