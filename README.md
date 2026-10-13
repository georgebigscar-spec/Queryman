# Queryman

Локальный HTTP-клиент для работы с API — аналог Postman без облака, аккаунтов и телеметрии.
Rust + egui + [shadcn_egui](https://github.com/georgebigscar-spec/shadcn_egui).

## Состояние

Этап 0 (каркас) и основная часть этапа MVP:

- окно на eframe с темой shadcn_egui, светлая и тёмная по системной настройке;
- UI-поток и рабочий поток tokio, связанные каналами `Command` / `Event`;
- вкладки запросов, метод, URL с синхронизацией таблицы Params;
- Headers, тело (none / raw / x-www-form-urlencoded), Auth (Inherit, None, API Key, Bearer, Basic);
- настройки запроса: таймаут, перенаправления, проверка TLS;
- отправка (Ctrl+Enter) и отмена (Esc), ответ: статус, время, размер, Pretty/Raw, заголовки;
- окружения и глобальные переменные `{{var}}`, динамические `{{$uuid}}`, `{{$timestamp}}`;
- копирование запроса как cURL;
- коллекции и папки на `shadcn_egui::Tree`: создание, переименование, дублирование, удаление,
  авторизация папки и коллекции для режима Inherit (контекстное меню по правой кнопке);
- сохранение вкладки в коллекцию (Ctrl+S), несохранённые правки помечены `*` на вкладке;
- история отправленных запросов (до 500 записей, без тел ответов), повтор в новой вкладке;
- сохранение тела ответа в файл через системный диалог;
- автосохранение вкладок, окружений, коллекций и истории в JSON (атомарная запись, повреждённые файлы → `*.corrupt`).

Дальше по плану v1.0: импорт cURL и Postman, form-data и binary-тела, остальные виды авторизации.

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
├── queryman-io/        экспорт cURL (импорт cURL и Postman — этап v1.0)
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

## Проверка

CI (`.github/workflows/ci.yml`) запускает `cargo fmt --check`, `cargo clippy -D warnings` и `cargo test`.
shadcn_egui закреплён на конкретном коммите (`rev` в корневом `Cargo.toml`).
