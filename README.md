# PAProxy

[English](#english) | [Русский](#русский)

## English

PAProxy is a reverse proxy for PolyAnalyst 6 and PolyAnalyst Grid. It proxies
HTTP and WebSocket requests to an upstream PolyAnalyst server, serves selected
static files locally, and displays traffic statistics in a terminal UI.

### Features

- Supports PolyAnalyst 6 and PolyAnalyst Grid routes.
- Proxies HTTP, HTTPS, and WebSocket traffic to an upstream server.
- Optionally serves static application and help files locally.
- Can accept incoming HTTPS connections when a certificate and key are set.
- Shows request and traffic statistics in a terminal UI or, optionally, a
  graphical UI.
- Rotates application logs and can optionally log proxied HTTP headers.

### Requirements

- Rust toolchain (edition 2024; Rust 1.97 or later is recommended).
- Access to a PolyAnalyst upstream server.
- Local PolyAnalyst static files and help files when transparent mode is not
  used.

### Build

```powershell
cargo build --release
```

The resulting executable is `target/release/paproxy` (`paproxy.exe` on
Windows).

The graphical UI is optional and enabled by the `gui` feature:

```powershell
cargo build --release --features gui
```

On Linux the GUI build needs the X11/Wayland development packages, for example
on Debian or Ubuntu:

```sh
sudo apt-get install cmake clang pkg-config libfontconfig-dev libfreetype-dev   libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev   libxcb1-dev libvulkan-dev
```

To build the release binaries, use:

```powershell
just release
```

It produces in `build/`:

- `paproxy.exe` — Windows executable with the terminal and graphical UI.
- `paproxy` — static Linux executable with the terminal UI, built in Docker.
- `paproxy-gui` — Linux executable with the terminal and graphical UI, built in
  Docker. It requires glibc, libxcb, libxkbcommon, libxkbcommon-x11 and a
  Vulkan driver, which a Linux desktop usually has.

The application icon is in `assets/icons`; the Windows executable embeds it.
To add PAProxy with its icon to a Linux desktop menu, put `paproxy-gui` on
`PATH` and run from the repository:

```sh
cp -r assets/icons/hicolor ~/.local/share/icons/
cp assets/linux/paproxy.desktop ~/.local/share/applications/
```

### Configuration

Copy `config.yml.example` to `config.yml` and update its values:

```yaml
port: 3000
https_port: 3001
sourcedata: D:\pa\SourceData\www
help: D:\pa\SourceData\www\help
host: 192.168.1.10:5043
pagrid: false
certificate_path:
key_path:
log_headers: false
transparent: false
insecure_upstream: true
upstream_ca_path:
```

| Option | Description |
| --- | --- |
| `port` | Local HTTP listening port. |
| `https_port` | Local HTTPS listening port. Required only when both TLS paths are set. |
| `sourcedata` | Path to local static application files. Not required with `--transparent`. |
| `help` | Path to local help files. Not required with `--transparent`. |
| `host` | Upstream PolyAnalyst address in `host:port` form. PAProxy connects to it over HTTPS/WSS. |
| `pagrid` | Selects PolyAnalyst Grid routes when `true`; selects PolyAnalyst 6 routes when `false` (default). |
| `certificate_path` | PEM certificate-chain path for the incoming HTTPS server. Leave empty to disable it. |
| `key_path` | PEM private-key path for the incoming HTTPS server. Leave empty to disable it. |
| `log_headers` | When `true`, records proxied HTTP request and response headers in `logs/headers.json`. Defaults to `false`. |
| `transparent` | When `true`, proxies static routes too instead of serving them from `sourcedata` and `help`, like `--transparent`. Defaults to `false`. |
| `insecure_upstream` | When `true` (default), accepts any upstream TLS certificate, including self-signed ones. When `false`, verifies it against system CA certificates and `upstream_ca_path`. |
| `upstream_ca_path` | PEM file with additional CA certificates trusted for the upstream server. Used only when `insecure_upstream` is `false`. |

`config.yml` is ignored by Git, so credentials and environment-specific paths
can remain local. You may use another YAML file with `--config`.

> **Security note:** `log_headers: true` may record cookies, authorization
> tokens, and other sensitive values. Enable it only for troubleshooting and
> protect or remove the resulting logs afterwards.

> **Security note:** with the default `insecure_upstream: true` the connection
> to PolyAnalyst is encrypted but not authenticated, so it can be intercepted on
> the network. If the upstream certificate is issued by a trusted or internal
> CA, set `insecure_upstream: false` and, for an internal CA, `upstream_ca_path`.

### Run

```powershell
# Uses ./config.yml
.\target\release\paproxy.exe

# Uses another configuration file and sets the log level
.\target\release\paproxy.exe --config .\config.dev.yml --loglevel debug
```

Available options:

```text
--config <PATH>     Path to a YAML configuration file
--loglevel <LEVEL>  Log level, for example: error, warn, info, debug, trace
-t, --transparent   Proxy static routes instead of serving local static files
--tui               Run with the terminal UI
--headless          Run without a UI and write logs to stdout
```

Builds with the `gui` feature open the graphical UI by default; use `--tui` or
`--headless` to run them in a console. Builds without it start the terminal UI.

The graphical UI does not start the proxy by itself. It loads the settings,
lets you edit them and press **Start**; paths can be picked with the folder
button next to their fields. Settings are locked while the proxy runs; press
**Stop** to change them.

Settings are kept in profiles, for example one per PolyAnalyst server:

- **Default** is the configuration file itself. If it is missing or invalid,
  the window starts with default values.
- Other profiles are files of the same format in the `profiles` directory next
  to the configuration file, so any of them also works with `--config`, for
  example `--config profiles/stage.yml`.
- The profile list switches between them, **Save** writes the settings into the
  selected profile keeping its comments, **Save as** creates a new profile with
  the entered name, and the delete button removes the selected one.
- The window reopens the last selected profile.

Application logs are stored under `logs/` and rotated by size. Traffic
statistics are written to `logs/results.csv` when the proxy stops. Close the
window or press `Ctrl+C` to stop the proxy gracefully.

Use `--headless` when no terminal is available, for example in Docker or when
running PAProxy as a service. The terminal UI refuses to start outside a
terminal.

### Docker

```powershell
docker build -t paproxy .
docker run --rm -p 3000:3000 -v ${PWD}/config.yml:/config.yml paproxy --headless --config /config.yml
```

Mount any directories referenced by `sourcedata`, `help`, `certificate_path`,
and `key_path` as needed. Use Linux paths in the configuration used inside the
container. To use the terminal UI instead, replace `--headless` with the `-it`
flags of `docker run`.

## Русский

PAProxy — обратный прокси-сервер для PolyAnalyst 6 и PolyAnalyst Grid. Он
перенаправляет HTTP- и WebSocket-запросы на сервер PolyAnalyst, при необходимости
раздаёт локальные статические файлы и показывает статистику трафика в терминале
или в графическом интерфейсе.

### Возможности

- Поддержка маршрутов PolyAnalyst 6 и PolyAnalyst Grid.
- Проксирование HTTP, HTTPS и WebSocket-трафика на upstream-сервер.
- Локальная раздача статических файлов приложения и справки.
- Входящий HTTPS при настройке сертификата и ключа.
- Терминальный или, по желанию, графический интерфейс со статистикой запросов и
  трафика.
- Ротация журналов и опциональное логирование HTTP-заголовков.

### Требования

- Rust (edition 2024; рекомендуется Rust 1.97 или новее).
- Сетевой доступ к upstream-серверу PolyAnalyst.
- Локальные статические файлы и файлы справки PolyAnalyst, если не используется
  режим `--transparent`.

### Сборка

```powershell
cargo build --release
```

Исполняемый файл будет создан в `target/release/paproxy` (`paproxy.exe` в
Windows).

Графический интерфейс необязателен и включается фичей `gui`:

```powershell
cargo build --release --features gui
```

В Linux для сборки с GUI нужны пакеты разработки X11/Wayland, например в Debian
или Ubuntu:

```sh
sudo apt-get install cmake clang pkg-config libfontconfig-dev libfreetype-dev   libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev   libxcb1-dev libvulkan-dev
```

Для сборки релизных исполняемых файлов используйте:

```powershell
just release
```

В каталоге `build/` появятся:

- `paproxy.exe` — исполняемый файл для Windows с терминальным и графическим
  интерфейсом.
- `paproxy` — статический исполняемый файл для Linux с терминальным интерфейсом,
  собирается в Docker.
- `paproxy-gui` — исполняемый файл для Linux с терминальным и графическим
  интерфейсом, собирается в Docker. Требует glibc, libxcb, libxkbcommon,
  libxkbcommon-x11 и драйвер Vulkan, которые обычно есть в Linux с рабочим столом.

Иконка приложения лежит в `assets/icons`, в исполняемый файл для Windows она
встроена. Чтобы добавить PAProxy с иконкой в меню рабочего стола Linux, положите
`paproxy-gui` в каталог из `PATH` и выполните из корня репозитория:

```sh
cp -r assets/icons/hicolor ~/.local/share/icons/
cp assets/linux/paproxy.desktop ~/.local/share/applications/
```

### Настройка

Скопируйте `config.yml.example` в `config.yml` и задайте значения:

```yaml
port: 3000
https_port: 3001
sourcedata: D:\pa\SourceData\www
help: D:\pa\SourceData\www\help
host: 192.168.1.10:5043
pagrid: false
certificate_path:
key_path:
log_headers: false
transparent: false
insecure_upstream: true
upstream_ca_path:
```

| Параметр | Описание |
| --- | --- |
| `port` | Локальный порт HTTP-сервера. |
| `https_port` | Локальный порт HTTPS-сервера. Обязателен, только если заданы оба TLS-пути. |
| `sourcedata` | Путь к локальным статическим файлам приложения. Не нужен в режиме `--transparent`. |
| `help` | Путь к локальным файлам справки. Не нужен в режиме `--transparent`. |
| `host` | Адрес upstream-сервера PolyAnalyst в виде `host:port`. PAProxy подключается к нему по HTTPS/WSS. |
| `pagrid` | При `true` включаются маршруты PolyAnalyst Grid, при `false` (по умолчанию) — PolyAnalyst 6. |
| `certificate_path` | Путь к PEM-цепочке сертификатов для входящего HTTPS. Оставьте пустым, чтобы отключить HTTPS. |
| `key_path` | Путь к PEM-приватному ключу для входящего HTTPS. Оставьте пустым, чтобы отключить HTTPS. |
| `log_headers` | При `true` записывает заголовки проксируемых HTTP-запросов и ответов в `logs/headers.json`. По умолчанию `false`. |
| `transparent` | При `true` проксирует и статические маршруты вместо раздачи из `sourcedata` и `help`, как `--transparent`. По умолчанию `false`. |
| `insecure_upstream` | При `true` (по умолчанию) принимается любой TLS-сертификат upstream-сервера, в том числе самоподписанный. При `false` сертификат проверяется по системным корневым сертификатам и `upstream_ca_path`. |
| `upstream_ca_path` | Путь к PEM-файлу с дополнительными корневыми сертификатами для проверки upstream-сервера. Используется, только если `insecure_upstream` равен `false`. |

`config.yml` исключён из Git, поэтому в нём можно хранить локальные пути и
секреты. Другой YAML-файл можно передать параметром `--config`.

> **Важно:** при `log_headers: true` в журнал могут попасть cookie, токены
> авторизации и другие конфиденциальные данные. Включайте опцию только для
> диагностики, а созданные логи защищайте или удаляйте после работы.

> **Важно:** при `insecure_upstream: true` (по умолчанию) соединение с
> PolyAnalyst шифруется, но подлинность сервера не проверяется, поэтому его
> можно перехватить в сети. Если сертификат upstream-сервера выпущен доверенным
> или внутренним центром сертификации, задайте `insecure_upstream: false`, а для
> внутреннего центра — ещё и `upstream_ca_path`.

### Запуск

```powershell
# Используется ./config.yml
.\target\release\paproxy.exe

# Указание другого файла конфигурации и уровня логирования
.\target\release\paproxy.exe --config .\config.dev.yml --loglevel debug
```

Параметры командной строки:

```text
--config <PATH>     Путь к YAML-файлу конфигурации
--loglevel <LEVEL>  Уровень логирования: error, warn, info, debug или trace
-t, --transparent   Проксировать статические маршруты вместо локальной раздачи файлов
--tui               Запустить терминальный интерфейс
--headless          Работать без интерфейса, журнал выводить в stdout
```

Сборки с фичей `gui` по умолчанию открывают графический интерфейс; для запуска
в консоли используйте `--tui` или `--headless`. Сборки без неё запускают
терминальный интерфейс.

Графический интерфейс не запускает прокси сам. Он загружает настройки,
позволяет их изменить и запустить прокси кнопкой **Start**; пути можно выбрать
кнопкой с папкой рядом с полем. Пока прокси работает, настройки заблокированы;
чтобы изменить их, нажмите **Stop**.

Настройки хранятся в профилях, например по одному на сервер PolyAnalyst:

- **Default** — сам файл конфигурации. Если его нет или он некорректен, окно
  открывается со значениями по умолчанию.
- Остальные профили — файлы того же формата в каталоге `profiles` рядом с файлом
  конфигурации, поэтому любой из них подходит и для `--config`, например
  `--config profiles/stage.yml`.
- Список профилей переключает их, **Save** записывает настройки в выбранный
  профиль с сохранением комментариев, **Save as** создаёт новый профиль с
  введённым именем, кнопка удаления удаляет выбранный.
- Окно открывается с последним выбранным профилем.

Журналы приложения хранятся в `logs/` и ротируются по размеру. Статистика
трафика записывается в `logs/results.csv` при остановке прокси. Для корректной
остановки закройте окно или нажмите `Ctrl+C`.

Используйте `--headless`, если терминала нет, например в Docker или при запуске
PAProxy как службы. Терминальный интерфейс вне терминала не запустится.

### Docker

```powershell
docker build -t paproxy .
docker run --rm -p 3000:3000 -v ${PWD}/config.yml:/config.yml paproxy --headless --config /config.yml
```

При необходимости также смонтируйте каталоги из `sourcedata`, `help`,
`certificate_path` и `key_path`. В конфигурации внутри контейнера используйте
Linux-пути. Чтобы работать с терминальным интерфейсом, замените `--headless` на
флаги `-it` команды `docker run`.
