---
type: JS Module
title: stryker.config.mjs
resource: relay/stryker.config.mjs
docgen:
  crc: c9c61df7
  model: openai-codex/gpt-5.4-mini
  score: 100
  issues: judge:inaccurate:0.97
  judgeModel: openai-codex/gpt-5.4-mini
---

## Огляд

Файл запускає mutation testing для пов’язаних із зміненою ділянкою коду тестів, щоб швидше показувати результат без повного прогону suite. Конфіги, на які спирається код: mutation.json, incremental.json. Він також веде службові артефакти в reports/stryker/.tmp і формує reports/stryker/mutation.json та reports/stryker/incremental.json, щоб зберігати підсумок запуску й продовжувати інкрементальні обчислення між прогонами.

## Поведінка

1. Запускає mutation testing через `vitest`, щоб вимірювати якість тестового покриття на рівні мутованих змін.
2. Перевіряє лише ті тести, що стосуються зміненої ділянки коду, щоб швидше отримувати результат без зайвих прогонів усього suite.
3. Зберігає службові артефакти виконання в окремій тимчасовій теці `reports/stryker/.tmp`, щоб не змішувати їх із робочими файлами проєкту.
4. Формує два вихідні конфіги: `reports/stryker/mutation.json` для підсумку mutation testing і `reports/stryker/incremental.json` для відновлення попереднього стану між запусками.
5. Працює в incremental-режимі, щоб продовжувати попередні обчислення після переривання або аварійного завершення.
6. Не описує і не гарантує обробку шляхів поза зазначеними артефактами `mutation.json` і `incremental.json`.

## Гарантії поведінки

- (специфічних машинно-виведених гарантій немає)
