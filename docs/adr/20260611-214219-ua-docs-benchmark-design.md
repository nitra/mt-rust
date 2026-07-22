## ADR UA docs benchmark — підхід до оцінки якості та продуктивності LLM

## Context and Problem Statement

Потрібно порівняти кілька MLX-варіантів Gemma 4 E2B (uniform PTQ, QAT, OptiQ) за якістю генерації технічної документації українською мовою та за швидкістю інференсу на локальному oMLX-сервері (Apple M2 8 GB RAM).

## Considered Options

* 5 промптів × 4 перевірки якості + tok/s + RAM diff на першому запиті, з послідовним вивантаженням між моделями
* Інші варіанти в transcript не обговорювалися.

## Decision Outcome

Chosen option: "5 промптів × 4 перевірки якості + tok/s + RAM diff, sequential unload", because це покриває репрезентативні категорії технічної документації (REST endpoint, JSDoc, architecture, error catalog, config reference) і дозволяє запустити всі три моделі послідовно на машині з 8 GB RAM.

**Деталі реалізації:**

- **Якість** — 4 `VERDICT_CHECKS` на кожну відповідь: (1) частка українських слів ≥ 30%, (2) наявність Markdown (`#` або ` ``` `), (3) відповідь не є суто англійською (< 20% англ. слів), (4) наявність технічного контенту (`api|endpoint|config|error` тощо).
- **Швидкість** — `completion_tokens / elapsed_sec` → `tok/s`; вимірюється для кожного промпту.
- **RAM** — `vm_stat` до першого запиту кожної моделі; різниця між замірами = приблизний footprint завантаженої моделі.
- **Кеш-bust** — унікальний `RUN_SEED = random.randint(0, 2**31 - 1)` передається у `"seed"` кожного `chat()`-запиту, щоб oMLX не повертав закешовану відповідь.
- **Retry** — при `IncompleteRead` або мережевій помилці `chat()` повторює до 3 разів з паузою 3 с.
- **Вивантаження між моделями** — `POST /v1/models/{id}/unload` + 5 с очікування; наступна модель завантажується oMLX автоматично на першому запиті.
- **Memory guard** — `memory_guard_tier: balanced`; моделі що не вміщаються отримують ПОМИЛКА і не враховуються в таблиці (не aborting run).

### Consequences

* Good, because transcript фіксує очікувану користь: перша модель (4bit) дала 20/20 score і реальний ~226 t/s; qat дала 4/20 (перший промпт пройшов, решта — IncompleteRead від memory pressure), що підтвердило необхідність retry.
* Bad, because RAM-замір через `vm_stat` дає шум від інших процесів; різниця між замірами показує приблизний footprint моделі, а не точний.

## More Information

- `docs/omlx-ua-docs-bench.py` — основний benchmark-скрипт
- `docs/run-bench.sh` — оболонка: перевіряє `memory_guard_tier`, рестартує oMLX якщо потрібно, запускає benchmark
- `~/.omlx/model_settings.json` — `ttl_seconds: 300`, `enable_thinking: false` для всіх трьох моделей (persistent, не перезаписується при рестарті)
- Unload API: `POST http://127.0.0.1:8000/v1/models/{model_id}/unload`
- Admin login: `POST http://127.0.0.1:8000/admin/api/login` з cookie (`-c /tmp/omlx-bench-cookies.txt`)
- `enable_thinking: false` критично — без цього модель генерує chain-of-thought англійською і не доходить до українського контенту в межах `max_tokens`
