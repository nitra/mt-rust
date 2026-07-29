# Gix migration design

## Goal

Перевести всі Git-взаємодії `mt-rust`, для яких `gix` має еквівалентну й
достатньо безпечну семантику, із запуску системного `git` на Rust API `gix`.
Зберегти поточні гарантії claims, recovery, worktree isolation і fenced
publish. Системний Git допускається лише за одним ізольованим інтерфейсом для
операцій, які `gix` на момент реалізації не підтримує на потрібному рівні.

## Контекст

Зараз production-код викликає `git` через `std::process::Command` у
`mt-core::{claims,worktree,publish,runner}` і частково в CLI. Git зберігає
стан розподіленого виконання у remote refs:

- `refs/mt/claims/<node-hash>` — CAS-claim, commit містить `.mt-claim.yml`;
- `refs/mt/runs/<node-hash>/<token>` — recovery/handoff checkpoint;
- `refs/heads/main` — результат fenced publish.

`gix` 0.86 надає repository discovery, refs, object database, commit/tree
creation, index/status, fetch/push transport і читання linked-worktree
metadata. Він не надає готового API з еквівалентною семантикою для створення,
видалення та prune linked worktrees, повного rebase або atomic multi-ref push.

## Рішення

Ввести `mt-core::git` як єдину facade над Git. Код поза цим модулем не має
імпортувати `gix` і не має запускати `Command::new("git")`. Facade поділяється
за можливостями, а не за нинішніми файлами:

```text
mt-core::git
├── Repository        discovery, origin, refs, objects, commits, status
├── Remote            list refs, fetch, push одного/кількох ref updates
├── Worktree          inspection через gix
└── compat            тільки worktree add/remove/prune, rebase, atomic push
```

`compat` є внутрішнім, забороненим для прямого використання production-кодом.
Усі його методи мають назви за семантикою (`create_linked_worktree`,
`rebase_onto`, `push_atomic`) і задокументовану причину fallback. Це не
узагальнений "run arbitrary git" API.

## Capability matrix

| Поточна можливість | Цільовий механізм | Вимога до семантики |
| --- | --- | --- |
| Discover repo, root, `origin` | `gix::discover`, `Repository` config/remote | Працює з main і linked worktree. |
| Read/list local refs, resolve SHA | `gix` refs/object API | Повні ref names; без shell parsing. |
| Read remote claim refs | `gix` remote listing/fetch | Отримує тільки `refs/mt/claims/*`; errors не маскуються як empty state. |
| Create claim commit | `gix` object writer + ref transaction | Створює blob/tree/commit без index або checkout. |
| Claim acquire/renew/release | `gix` transport/ref updates | Зберігає compare-and-swap поведінку `--force-with-lease`. |
| Run ref checkpoint/delete | `gix` transport/ref updates | Пушить/видаляє exact custom ref. |
| Commit worktree result | `gix` index/status/commit API | Зберігає author/committer `mt-runner`; no-op на чистому дереві. |
| Worktree list/inventory | `gix` linked-worktree inspection | Парсинг porcelain output зникає. |
| Worktree create/remove/prune | `compat` | Зберігає Git адміністративні записи й `--force`. |
| Rebase result worktree | `compat` | Конфлікт завершує publish, rebase abort виконується. |
| Fenced atomic publish | `compat` | Один atomic push: `main` update + CAS-delete claim/run refs. |
| Test fixture Git setup | `gix` test support | Створює bare remote, clone, commits, refs без CLI. |
| CI/release shell commands | `gix` або non-Git tooling, де це runtime Rust | GitHub Actions checkout/push release workflow лишається GitHub Actions concern; не є Rust runtime. |

## Безпека й коректність

Claims і publish не можуть деградувати до read-then-write без remote CAS.
Якщо `gix` transport API не забезпечує exact expected-old-object для конкретної
операції, facade повертає capability error і викликає лише відповідний метод
`compat`; він не робить небезпечного best-effort push.

Перед publish worktree rebase-иться на точно зафіксований актуальний
`origin/main`. Успіх означає лише результат атомарної операції, що разом
оновила `main` і видалила exact claim/run refs. Частковий publish є помилкою.

Відкриття репозиторію використовує trust model `gix`; явна policy вирішує, чи
недовірений config є відмовою, а не непомітно запускає зовнішні helpers.

## API межі

`GitRepository::open(path)` повертає контекст для main або linked worktree.
Public methods приймають типізовані `FullRefName`, `ObjectId`, `ClaimRef` і
`RunRef`, а не сирі refspec strings. Конструювання `refs/mt/...` лишається в
одному модулі, який також валідовує node hash і token.

Remote update повертає один із трьох результатів:

- `Applied` — remote підтвердив очікувану зміну;
- `Rejected(LeaseMismatch)` — конкурентна гонка, нормальний доменний стан;
- `Err(GitError)` — transport, authentication, protocol або local I/O failure.

Це прибирає залежність від парсингу англомовного `git push` stderr.

## Міграційна послідовність

1. Додати facade й тестове середовище `gix` без зміни поведінки callers.
2. Перевести discovery, refs, object/claim commit і remote claim read.
3. Перевести acquire/renew/release claims та run refs, довівши CAS на локальному
   bare remote у паралельних тестах.
4. Перевести status/index/commit і worktree inspection.
5. Винести лишені shell-outs у `compat`; перевести worktree lifecycle, rebase
   та atomic publish на цей вузький API.
6. Замість CLI-based fixture helpers застосувати `gix`; залишити CLI лише в
   compat contract tests, що порівнюють результат Git porcelain.
7. Застосувати static guard: production crates не можуть містити
   `Command::new("git")`, а `compat` має allow-list трьох capability groups.
8. Після кожного оновлення `gix` перевіряти matrix; реалізований upstream API
   переносить відповідну capability з `compat` до native gix та видаляє fallback.

## Тестування

Unit-тести Git facade працюють із тимчасовими репозиторіями та локальним bare
remote, не з GitHub і не з checkout розробника. Обов'язкові сценарії:

- acquire одночасно двома runners: рівно один `Applied`;
- renew/release зі старим SHA: `Rejected(LeaseMismatch)`;
- claim commit має рівно `.mt-claim.yml`, коректний parent і поля;
- run ref recovery доступний після нового відкриття repo;
- чистий worktree не створює commit, брудний створює його з поточною ідентичністю;
- linked worktree inventory коректно відрізняє main, branch і detached run;
- compat worktree/rebase/atomic publish мають окремі інтеграційні тести;
- race publish не оновлює `main` і не видаляє чужий claim;
- статичний test знаходить заборонений shell-out поза `git::compat`.

Повний regression gate: `cargo test --workspace`, `cargo fmt --all -- --check`,
`cargo clippy --all-targets --all-features -- -D warnings`, репозиторний lint і
changelog gate.

## Позамежі

Міграція не змінює формат `.mt-claim.yml`, назви refs, протокол relay/ACP,
публічний CLI або логіку task graph. Вона також не замінює `actions/checkout`
у GitHub Actions: це environment provisioning, а не runtime Git-взаємодія
Rust-продукту.

## Критерій завершення

У production Rust-коді нема прямого `Command::new("git")`. Усі можливості,
наявні у pinned `gix`, реалізовані native API. Єдиний shell-out живе в
`mt-core::git::compat`, має allow-list worktree lifecycle, rebase і atomic
multi-ref push, покритий contract tests і задокументований capability matrix.
