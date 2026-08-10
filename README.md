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
- Shows request and traffic statistics in a terminal UI.
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

To build the native executable and the Docker image export, use:

```powershell
just release
```

### Configuration

Copy `config.yml.example` to `config.yml` and update its values:

```yaml
port: 3000
https_port: 3001
sourcedata: D:\\pa\\SourceData\\www
help: D:\\pa\\SourceData\\www\\help
host: 192.168.1.10:5043
pagrid: false
certificate_path:
key_path:
log_headers: false
```

| Option | Description |
| --- | --- |
| `port` | Local HTTP listening port. |
| `https_port` | Local HTTPS listening port. Used only when both TLS paths are set. |
| `sourcedata` | Path to local static application files. |
| `help` | Path to local help files. |
| `host` | Upstream PolyAnalyst address in `host:port` form. PAProxy connects to it over HTTPS/WSS. |
| `pagrid` | Selects PolyAnalyst Grid routes when `true`; selects PolyAnalyst 6 routes when `false`. |
| `certificate_path` | PEM certificate-chain path for the incoming HTTPS server. Leave empty to disable it. |
| `key_path` | PEM private-key path for the incoming HTTPS server. Leave empty to disable it. |
| `log_headers` | When `true`, records proxied HTTP request and response headers in `logs/headers.json`. |

`config.yml` is ignored by Git, so credentials and environment-specific paths
can remain local. You may use another YAML file with `--config`.

> **Security note:** `log_headers: true` may record cookies, authorization
> tokens, and other sensitive values. Enable it only for troubleshooting and
> protect or remove the resulting logs afterwards.

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
--transparent       Proxy static routes instead of serving local static files
```

Application logs are stored under `logs/` and rotated by size. Press `Ctrl+C`
to stop the proxy gracefully.

### Docker

```powershell
docker build -t paproxy .
docker run --rm -p 3000:3000 -v ${PWD}/config.yml:/config.yml paproxy --config /config.yml
```

Mount any directories referenced by `sourcedata`, `help`, `certificate_path`,
and `key_path` as needed. Use Linux paths in the configuration used inside the
container.

## Русский

PAProxy — обратный прокси-сервер для PolyAnalyst 6 и PolyAnalyst Grid. Он
перенаправляет HTTP- и WebSocket-запросы на сервер PolyAnalyst, при необходимости
раздаёт локальные статические файлы и показывает статистику трафика в терминале.

### Возможности

- Поддержка маршрутов PolyAnalyst 6 и PolyAnalyst Grid.
- Проксирование HTTP, HTTPS и WebSocket-трафика на upstream-сервер.
- Локальная раздача статических файлов приложения и справки.
- Входящий HTTPS при настройке сертификата и ключа.
- Терминальный интерфейс со статистикой запросов и трафика.
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

Для сборки нативного исполняемого файла и Docker-образа используйте:

```powershell
just release
```

### Настройка

Скопируйте `config.yml.example` в `config.yml` и задайте значения:

```yaml
port: 3000
https_port: 3001
sourcedata: D:\\pa\\SourceData\\www
help: D:\\pa\\SourceData\\www\\help
host: 192.168.1.10:5043
pagrid: false
certificate_path:
key_path:
log_headers: false
```

| Параметр | Описание |
| --- | --- |
| `port` | Локальный порт HTTP-сервера. |
| `https_port` | Локальный порт HTTPS-сервера. Используется, только если заданы оба TLS-пути. |
| `sourcedata` | Путь к локальным статическим файлам приложения. |
| `help` | Путь к локальным файлам справки. |
| `host` | Адрес upstream-сервера PolyAnalyst в виде `host:port`. PAProxy подключается к нему по HTTPS/WSS. |
| `pagrid` | При `true` включаются маршруты PolyAnalyst Grid, при `false` — PolyAnalyst 6. |
| `certificate_path` | Путь к PEM-цепочке сертификатов для входящего HTTPS. Оставьте пустым, чтобы отключить HTTPS. |
| `key_path` | Путь к PEM-приватному ключу для входящего HTTPS. Оставьте пустым, чтобы отключить HTTPS. |
| `log_headers` | При `true` записывает заголовки проксируемых HTTP-запросов и ответов в `logs/headers.json`. |

`config.yml` исключён из Git, поэтому в нём можно хранить локальные пути и
секреты. Другой YAML-файл можно передать параметром `--config`.

> **Важно:** при `log_headers: true` в журнал могут попасть cookie, токены
> авторизации и другие конфиденциальные данные. Включайте опцию только для
> диагностики, а созданные логи защищайте или удаляйте после работы.

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
--transparent       Проксировать статические маршруты вместо локальной раздачи файлов
```

Журналы приложения хранятся в `logs/` и ротируются по размеру. Для корректной
остановки прокси нажмите `Ctrl+C`.

### Docker

```powershell
docker build -t paproxy .
docker run --rm -p 3000:3000 -v ${PWD}/config.yml:/config.yml paproxy --config /config.yml
```

При необходимости также смонтируйте каталоги из `sourcedata`, `help`,
`certificate_path` и `key_path`. В конфигурации внутри контейнера используйте
Linux-пути.
