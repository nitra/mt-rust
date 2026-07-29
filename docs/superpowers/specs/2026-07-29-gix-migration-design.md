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
creation, fetch transport і читання linked-worktree metadata. У pinned версії
немає достатнього public high-level API, щоб відтворити `git add -A` та
index-only removal з тією ж семантикою. Також він не надає готового API з
exact remote lease для custom refs, створення/видалення/prune linked worktrees,
повного rebase або atomic multi-ref push.

## Рішення

Ввести `mt-core::git` як єдину facade над Git. Код поза цим модулем не має
імпортувати `gix` і не має запускати `Command::new("git")`. Facade поділяється
за можливостями, а не за нинішніми файлами:

```text
mt-core::git
├── Repository        discovery, origin, refs, objects, claim commits
├── Remote            list refs, fetch
├── Worktree          inspection через gix
└── compat            staging/commit, custom-ref CAS, worktree lifecycle,
                      branch config, rebase, atomic push і локальний ff
```

`compat` є внутрішнім, забороненим для прямого використання production-кодом.
Усі його методи мають назви за семантикою (`commit_all_if_changed`,
`push_with_expected`, `worktree`, `rebase_onto`, `push_atomic`) і
задокументовану причину fallback. Це не узагальнений "run arbitrary git" API.

## Capability matrix

| Поточна можливість | Цільовий механізм | Вимога до семантики |
| --- | --- | --- |
| Discover repo, root, `origin` | `gix::discover`, `Repository` config/remote | Працює з main і linked worktree. |
| Read/list local refs, resolve SHA | `gix` refs/object API | Повні ref names; без shell parsing. |
| Read remote claim refs | `gix` remote listing/fetch | Отримує тільки `refs/mt/claims/*`; errors не маскуються як empty state. |
| Create claim commit | `gix` object writer + ref transaction | Створює blob/tree/commit без index або checkout. |
| Claim acquire/renew/release | `compat` custom-ref CAS | Exact `--force-with-lease`; lease mismatch лишається доменним `false`. |
| Run ref checkpoint/delete | `compat` custom-ref push/CAS | Пушить/видаляє exact custom ref. |
| Commit worktree result | `compat` staging/commit | Зберігає author/committer `mt-runner`; no-op на чистому дереві. |
| Worktree list/inventory | `gix` linked-worktree inspection | Парсинг porcelain output зникає. |
| Worktree create/remove/prune | `compat` | Зберігає Git адміністративні записи й `--force`. |
| Rebase result worktree | `compat` | Конфлікт завершує publish, rebase abort виконується. |
| Fenced atomic publish | `compat` | Один atomic push: `main` update + CAS-delete claim/run refs. |
| Test fixture Git setup | `mt-core/test-support` + `gix` | Створює bare remote/worktree, читає refs/blobs і перевіряє remote advertisement без direct Git CLI. |
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
3. Для custom-ref CAS, status/index/commit залишити exact Git porcelain у
   `compat`, доки gix не надає еквівалентну безпечну API.
4. Перевести worktree inspection, runtime fetch і SHA resolution на gix;
   винести worktree lifecycle, rebase й atomic publish у `compat`.
5. Застосувати static guard: жоден Rust-файл у `crates/` не може містити
   `Command::new("git")` поза `git::compat`.
6. Після кожного оновлення `gix` перевіряти matrix; реалізований upstream API
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

У Rust-коді нема прямого `Command::new("git")` поза `git::compat`. Усі можливості,
для яких pinned `gix` 0.86 має еквівалентний безпечний API, реалізовані native
API. Єдиний production shell-out живе в `mt-core::git::compat`; його allow-list
охоплює staging/commit, custom-ref CAS, worktree lifecycle, branch config,
rebase, atomic multi-ref push і локальний fast-forward. Test fixtures доступні
лише за feature `mt-core/test-support` і використовують gix facade та
семантичні compat операції, не generic shell-out.
