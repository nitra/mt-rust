## ADR Адресація залежностей: відносний авторинг, абсолютне зберігання (Option 3)

## Context and Problem Statement
`deps/` файли з короткими іменами (`collect-data.md`) були неоднозначними: для вузла `quarterly-anomalies/analyze` ім'я `collect-data` могло означати і кореневий вузол `mt/collect-data/`, і сусіда `mt/quarterly-anomalies/collect-data/`.

## Considered Options
* Абсолютні dep-id від `mt/` (рекомендація рев'юера): `deps/quarterly-anomalies/collect-data.md`
* Відносні з `../` кодуванням у файловій системі (Option 2а/2б)
* Відносний авторинг у `## Children`, абсолютне зберігання в `deps/` після резолюції `mt spawn` (Option 3)
* YAML dep-descriptor з полем `node:` у вмісті файлу

## Decision Outcome
Chosen option: "Відносний авторинг, абсолютне зберігання (Option 3)", because оркестратор отримує однозначні абсолютні dep-id через `ls -R deps/` + strip `.md` без читання вмісту; авторинг залишається зручним через відносні імена у `## Children`; `mt spawn --approve` резолвить перед записом.

### Consequences
* Good, because оркестратор не читає вміст `deps/` файлів — сканування через `ls -R` достатнє.
* Good, because сусіди, нащадки і крос-рівневі залежності виражаються природно при авторингу.
* Bad, because `mt spawn --approve` і `mt init --deps` мають виконувати резолюцію відносних шляхів до абсолютних перед записом.

## More Information
Формула резолюції: `resolved = normalize(parent_path + "/" + dep_ref)`. `deps/` дзеркалює структуру `mt/` — filename є абсолютним dep-id від root. Вміст dep-файлу опційний (ref-нотатки для агента). Файл: `npm/docs/mt.md` рядки ~75, ~146, ~387, ~1362, ~1837.
