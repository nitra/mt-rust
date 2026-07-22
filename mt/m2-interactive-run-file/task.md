---
schema_version: 1
created_at: 2026-07-12T06:59:32Z
budget_sec: 7200
hint: atomic
---

## Task

Синтез контрактних артефактів в інтерактивному done: `run_NNN.md` (кожна спроба має run-файл — контракт graph.md) із секцією `## Approvals` (матеріалізація верифікованих підписів — access.md) і мінімальний `fact_NNN.md`, якщо виконавець його не створив (без fact вузол після publish не стає resolved — семантична дірка поточного done).

## Done when

- mt-core: `next_run_nnn`/`write_run_fm` публічні (одна реалізація формату run-файлу — нею користується і graph-міст agent-server);
- `InteractiveRun`: накопичення approval-рядків (`add_approval`); ws-гілка `ApprovalResponse` після успішної верифікації додає рядок (ts, device_id, approved, request_id, hex-підпис) у run вузла;
- `done()`: після `## Check` і фіксації run_ref SHA — синтез `run_NNN.md` (actor з конфігу, result success, `## Approvals` за наявності) + `fact_NNN.md` якщо відсутній → коміт → strip `.nitra/` → fenced publish;
- наявний fact виконавця з тим самим NNN НЕ перезаписується;
- тести: unit (done → main містить run_001/fact_001; approvals-рядок у run-файлі; власний fact збережено), інтеграційний graph_wiring оновлено (main після DoneSession містить run/fact);
- `cargo test --workspace` зелений.

## Check

cargo test -p agent-server -p mt-core -q

## Inputs

- Контракт: graph.md (`run_NNN.md` — спроба виконавця; `## Approvals` опційно; fact_NNN — успішний результат, NNN = NNN run-а), access.md (матеріалізація підписів у файли вузла).
- Побудовано на: ApprovalGate (PR #36), graph-міст done (PR #28/#32).
- Поза скоупом: телеметрія wall_sec/tokens/cost із ходів (окрема задача), audit-сигнал (`mt audit`) в інтерактиві.
