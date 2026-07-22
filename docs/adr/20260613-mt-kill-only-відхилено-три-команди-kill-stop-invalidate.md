## ADR `mt kill`-only підхід відхилено: три окремі команди kill / stop / invalidate

## Context and Problem Statement
Під час обговорення patch protocol було запропоновано спростити CLI, прибравши `mt stop` і `mt invalidate` на користь єдиної команди `mt kill`. У цьому варіанті patch protocol виглядав би: `mt kill analyze && mt kill synthesize` → патч → `mt init analyze && mt spawn --approve analyze` і т.д. Потрібно було оцінити, чи ця спрощуюча трейд-офф прийнятна.

## Considered Options
* `mt kill`-only: одна команда, проста CLI; topology відновлюється через `mt init` + `mt spawn --approve`
* Три окремі команди: `mt kill` (topology deletion) / `mt stop` (process halt без topology removal) / `mt invalidate` (execution reset, зберігає topology)

## Decision Outcome
Chosen option: "Три окремі команди", because `mt kill`-only руйнує три властивості: (1) differential cascade — після `mt init` descendants перестворюються з нуля й завжди виконуються заново незалежно від того, чи змінився hash upstream; (2) planning overhead — вузол заново проходить `mt plan`, і якщо план LLM-генерований або потребував human review, людина змушена апрувати вже схвалений план; (3) pause-семантика — engineer не може тимчасово зупинити вузол без руйнування topology (наприклад, для збору контексту або передачі іншому актору).

### Consequences
* Good, because transcript фіксує очікувану користь: differential cascade залишається ефективним; повторний human review при незмінній задачі не потрібен; topology зберігається між runs.
* Bad, because три команди з різними семантиками потребують чіткого документування розмежування між ними; `mt kill`-only залишається обґрунтованою альтернативою якщо differential cascade не потрібен і весь деferred cascade виключається зі специфікації.

## More Information
Змінений файл: `npm/docs/mt.md`. Рядок 1476: `mt kill <dep-node>` у Engineer protocol замінено на `mt stop <dep-node>` + `mt invalidate <dep-node>`. Рядок 1483: уточнено що `mt kill` — виключно остаточне видалення topology; основні інструменти engineer — `mt stop` і `mt invalidate`. `mt kill`-only як підхід явно позначений як свідома спрощуюча трейд-офф що обнуляє deferred cascade.
