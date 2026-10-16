# Queryman

Локальный HTTP-клиент для работы с API — аналог Postman без облака, аккаунтов и телеметрии.
Rust + egui + [shadcn_egui](https://github.com/georgebigscar-spec/shadcn_egui).

## Состояние

Этап 0 (каркас) и основная часть этапа MVP:

- окно на eframe с темой shadcn_egui, светлая и тёмная по системной настройке;
- UI-поток и рабочий поток tokio, связанные каналами `Command` / `Event`;
- вкладки запросов, метод, URL с синхронизацией таблицы Params;
- Headers, тело (none / raw / x-www-form-urlencoded / form-data с файлами / binary), Auth (Inherit, None, API Key, Bearer, Basic);
- настройки запроса: таймаут, перенаправления, проверка TLS;
- отправка (Ctrl+Enter) и отмена (Esc), ответ: статус, время, размер, Pretty/Raw, заголовки;
- окружения и глобальные переменные `{{var}}`, динамические `{{$uuid}}`, `{{$timestamp}}`;
- копирование запроса как cURL;
- импорт (кнопка «Импорт» в боковой панели): команда cURL (bash или cmd), коллекция Postman v2.0/v2.1
  с папками, авторизацией, телами и скриптами, окружение Postman. Переменные коллекции становятся окружением.
  Команду cURL можно вставить прямо в поле URL;
- коллекции и папки на `shadcn_egui::Tree`: создание, переименование, дублирование, удаление,
  авторизация папки и коллекции для режима Inherit (контекстное меню по правой кнопке);
- сохранение вкладки в коллекцию (Ctrl+S), несохранённые правки помечены `*` на вкладке;
- история отправленных запросов (до 500 записей, без тел ответов), повтор в новой вкладке;
- сохранение тела ответа в файл через системный диалог;
- палитра команд (Ctrl+K): действия, переключение окружений и вкладок, поиск по всем сохранённым запросам (название и URL);
- поиск окружения и папки для сохранения в выпадающих списках, поиск по истории;
- скрипты pre-request и tests на JavaScript (встроенный QuickJS, вкладка Scripts): объект `pm` как в Postman —
  `pm.environment` / `pm.globals` / `pm.variables`, `pm.request` (метод, URL, заголовки, тело), `pm.response`,
  `pm.test`, `pm.expect` (основные утверждения chai), `console.log`; результаты — во вкладках Tests и Console ответа.
  Каждый скрипт ограничен 5 секундами и 64 МБ памяти;
- автосохранение вкладок, окружений, коллекций и истории в JSON (атомарная запись, повреждённые файлы → `*.corrupt`).

Дальше по плану v1.0: остальные виды авторизации (Digest, OAuth 2.0, JWT, AWS SigV4).

## Сборка

```sh
cargo run -p queryman-app            # debug
cargo build -p queryman-app --release
cargo test --workspace               # тесты без доступа к сети (wiremock на localhost)
```

Шрифты Inter и JetBrains Mono: положите TTF в `assets/fonts/` и соберите с `--features bundled-fonts`.

На Linux для eframe нужны пакеты разработки X11/Wayland (например, на Ubuntu:
`libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev`). Готовый бинарник их не требует.

## Структура

```
crates/
├── queryman-core/      модель, переменные, URL ↔ Params, подготовка запроса, протокол
├── queryman-http/      исполнитель на reqwest (rustls), пул клиентов, cookie jar
├── queryman-storage/   JSON-файлы, schema_version, атомарная запись
├── queryman-script/    скрипты pre-request/tests на QuickJS, API pm.* (prelude.js)
├── queryman-io/        экспорт cURL, импорт cURL и Postman
└── queryman-app/       eframe, UI, рабочий поток
    └── src/ui/components.rs — единственное место вызовов shadcn_egui
```

## Данные

- Linux: `~/.local/share/queryman/`
- macOS: `~/Library/Application Support/queryman/`
- Windows: `%APPDATA%\queryman\data\`
- Внутри: `workspace.json`, `globals.json`, `history.json`, `environments/<uuid>.json`, `collections/<uuid>.json`.
- Портативный режим: файл `portable` рядом с исполняемым файлом → данные в `./data/`.

Логи: `RUST_LOG=debug cargo run -p queryman-app`.

## Готовые сборки

Workflow `Build` собирает release-бинарники для Linux (x86_64, `.tar.gz`) и Windows (x86_64, `.zip`)
на каждый push в `main` и в PR — они лежат в артефактах запуска на вкладке Actions.
Тег `v*` (например, `git tag v0.1.0 && git push origin v0.1.0`) дополнительно публикует GitHub Release с этими архивами.

## Проверка

CI (`.github/workflows/ci.yml`) запускает `cargo fmt --check`, `cargo clippy -D warnings` и `cargo test`.
shadcn_egui закреплён на конкретном коммите (`rev` в корневом `Cargo.toml`).
