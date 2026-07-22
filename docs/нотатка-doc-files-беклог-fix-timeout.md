# Нотатка: беклог файлових док і fix-timeout у `lint doc-files`

**Дата:** 2026-07-07
**Upstream issue:** [nitra/cursor#16](https://github.com/nitra/cursor/issues/16)

## Суть

Перший масовий прогін `npx @nitra/cursor lint doc-files` (беклог 47 відсутніх док, локальна
модель `omlx/gemma-4-e2b-it-4bit`, ~40–56s/файл) не сходиться: fix-pipeline пакета
`@nitra/cursor` (14.x) обгортає весь батч worker-а backstop-таймаутом 56250ms
(`N_LOCAL_FIX_TIMEOUT_MS=45000` × 1.25), а після `fix timeout` central rollback
(`snapshot.rollback()`) видаляє **всі** вже записані доки — повторний прогін щоразу
стартує з `[1/47]`. Деталі й ланцюжок причини — в issue.

## Як згенеровано беклог тут

Прямим batch-CLI поза fix-pipeline (інкрементний, resumable, без rollback):

```bash
node node_modules/@nitra/cursor/rules/doc-files/docgen-files-batch/main.mjs gen
```

Після нього `npx @nitra/cursor lint doc-files` бачить свіжі CRC і проходить чисто.

## ⚠ Колізія: `npm/docs/index.md` — людська дока

Схема `<dir>/docs/<stem>.md` мапить `npm/index.js` → `npm/docs/index.md`, а це **рукописний**
зміст документації модуля. Перший прогін генерації мовчки перезаписав його; файл відновлено
з git і йому додано мінімальний `docgen`-frontmatter (`resource: npm/index.js` + свіжий `crc`),
щоб сканер вважав доку свіжою. **Латентний ризик:** після зміни `npm/index.js` CRC розійдеться
і наступний прогін doc-files знову перезапише людський файл — до фіксу upstream
([nitra/cursor#16](https://github.com/nitra/cursor/issues/16), коментар 3) після зміни
`npm/index.js` онови `crc` у frontmatter вручну (`crc32` джерела) або перенеси людський зміст.

## Важелі на майбутнє (поки issue не закрито)

- `N_LOCAL_FIX_TIMEOUT_MS` — env-override local-таймауту fix-рунга (недокументований);
  для великого масового прогону через `lint doc-files` треба ~60000ms × кількість файлів, і навіть
  тоді один transient-збій відкочує весь прогін.
- Локальна модель одна на машину: конкурентний `adr-normalize`-батч розтягує
  docgen-виклики за 300s → transient-помилки. Великий прогін краще запускати, коли
  ADR-нормалізація не працює.
