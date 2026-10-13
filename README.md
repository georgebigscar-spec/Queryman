# Queryman

Локальный HTTP-клиент для работы с API — аналог Postman без облака, аккаунтов и телеметрии.
Rust + egui + [shadcn_egui](https://github.com/georgebigscar-spec/shadcn_egui).

## Состояние

Этап 0 (каркас) плюс сквозной срез, чтобы каркас можно было проверить руками:

- окно на eframe с темой shadcn_egui, светлая и тёмная по системной настройке;
- UI-поток и рабочий поток tokio, связанные каналами `Command` / `Event`;
- вкладки запросов, метод, URL с синхронизацией таблицы Params;
- Headers, тело (none / raw / x-www-form-urlencoded), Auth (Inherit, None, API Key, Bearer, Basic);
- настройки запроса: таймаут, перенаправления, проверка TLS;
- отправка (Ctrl+Enter) и отмена (Esc), ответ: статус, время, размер, Pretty/Raw, заголовки;
- окружения и глобальные переменные `{{var}}`, динамические `{{$uuid}}`, `{{$timestamp}}`;
- копирование запроса как cURL;
- автосохранение вкладок и окружений в JSON (атомарная запись, повреждённые файлы → `*.corrupt`).

Следующий шаг — этап MVP: коллекции на `shadcn_egui::Tree`, история, сохранение ответа в файл.

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
- Портативный режим: файл `portable` рядом с исполняемым файлом → данные в `./data/`.

Логи: `RUST_LOG=debug cargo run -p queryman-app`.

## Первая сборка: на что обратить внимание

Код написан без компиляции. Вероятные места правок:

1. `src/ui/components.rs` — сигнатуры shadcn_egui (`toast`, `tabs`, `toggle_group`, `Input`).
   Все вызовы собраны в этом файле.
2. API egui/eframe 0.36: `selectable_label`, `ComboBox::from_id_salt`, `DragValue::range`.
3. После успешной сборки закрепите коммит shadcn_egui (`rev = "…"` в корневом `Cargo.toml`).
