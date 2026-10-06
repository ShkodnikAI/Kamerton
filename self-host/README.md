# Self-hosting Камертона на домашнем сервере (наряд Н1-02)

Инструкция владельца. Домашний сервер — Windows 10 под WSL2, работает 24/7,
аренда не нужна. Приложение целиком на Metalogos: поднимается
`mlog serve app.mlog` под systemd внутри WSL. База живёт на `/data`,
еженедельная копия уходит на внешний носитель. Публичный адрес и открытые
порты не нужны: Telegram-цикл — long polling, машина сама опрашивает сеть.

Наряд: issue **#21** (волна 1). Артефакты деплоя: `deploy/`. Всё здесь —
host-скрипты и инструкция, самого кода приложения они не содержат.

---

## 1. Шаг 0 — снять факты о машине

В PowerShell на Windows-хосте:

```powershell
powershell -ExecutionPolicy Bypass -File self-host\step0-facts.ps1
```

Скрипт ничего не меняет: печатает версию Windows и сборку, версию WSL
(`wsl --version`, `--status`, список дистрибутивов), доступные состояния
сна (`powercfg /a`) и текущие таймауты сна/гибернации/диска. Вывод вставь
в комментарий issue #21 — это фиксация Шага 0 наряда.

## 2. Убедиться, что машина не уходит в сон и не гасит диск

```powershell
powershell -ExecutionPolicy Bypass -File self-host\disable-sleep.ps1
```

Скрипт выставляет `standby-timeout-ac 0`, `hibernate-timeout-ac 0`,
`disk-timeout-ac 0` — система и диск не засыпают от сети, монитор гаснуть
может. После него `self-host\step0-facts.ps1` должен показать `0` по всем
трём таймаутам, а `powercfg /a` — не показывать ждущий режим как активный.
Если машина должна переживать отключение питания — гибернация
отключена сознательно: сервер либо работает, либо перезагружается, и
приёмочный тест проверяет именно автоподъём после перезагрузки.

## 3. WSL2 с Ubuntu и systemd

Если WSL ещё не установлен: `wsl --install -d Ubuntu` в PowerShell от
администратора, затем перезагрузка. `deploy/install.sh` сам допишет в
`/etc/wsl.conf` блок `[boot] systemd=true` — без него systemd-юниты не
поднимутся. Проверка после установки:

```bash
wsl.exe --status          # версия WSL, дистрибутив по умолчанию
cat /etc/wsl.conf         # после install.sh: [boot] systemd=true
```

Заметка о часах: WSL2 после сна/возобновления хоста может отставать —
инсталлятор включает `systemd-timesyncd`, cron-тики и утреннее окно 7:00
зависят от корректного времени.

## 4. Установка приложения

Чекаут репозитория и установка (идемпотентна — повторный запуск ничего не
ломает; `DRY_RUN=1` только печатает действия):

```bash
sudo bash deploy/install.sh
```

Инсталлятор делает: `/data/kamerton` (700, владелец — твой пользователь);
`/data/kamerton/env` (600: пустые `KAMERTON_TG_TOKEN` и `KAMERTON_CHAT_ID`,
путь к базе, `KAMERTON_TZ_OFFSET=3`, `METALOGOS_ENV_ALLOWLIST` — выводится
из `deploy/env.allowlist`); бинарник `mlog v0.28.2` с проверкой SHA256;
`app.mlog` (пока Н1-06 не смержен — ставится `deploy/smoke.mlog`
заглушкой); `backup.sh`; `sqlite3`; systemd-юниты с подстановкой твоего
пользователя в `User=`; enable + старт.

Заполни секреты (внутри WSL):

```bash
sudo nano /data/kamerton/env   # KAMERTON_TG_TOKEN=..., KAMERTON_CHAT_ID=...
sudo systemctl restart kamerton.service
```

Токен бота создаёт @BotFather; chat id — свой числовой id у @userinfobot.
Токен живёт только в `/data/kamerton/env` (600) — в коде, репозитории и
issues ему делать нечего (закон, §4).

## 5. Внешний носитель для бэкапа

Бэкап-скрипт пишет в `/mnt/external/kamerton-backup` и громко отказывается
работать, если диск не смонтирован. Подключи USB-диск к Windows и
смонтируй его букву в WSL2, добавив в `/etc/fstab` внутри WSL:

```
E: /mnt/external drvfs defaults,noatime,uid=1000,gid=1000 0 0
```

(букву `E:` и uid/gid подставь свои), затем `sudo mkdir -p /mnt/external
&& sudo mount -a`. Другой путь — тот же `mount -t drvfs E: /mnt/external`
руками перед запуском таймера; таймер запускается в воскресенье 03:00,
`Persistent=true` догоняет пропущенный запуск после простоя.

## 6. Автостарт после перезагрузки Windows

```powershell
powershell -ExecutionPolicy Bypass -File self-host\install-autostart.ps1
```

Создаёт задачу планировщика **«Kamerton WSL boot»** (при старте системы,
от SYSTEM, с задержкой 30 с, три рестарта при сбое): `wsl.exe -d Ubuntu
-u root -- systemctl start kamerton.service kamerton-backup.timer`. С
systemd=true в `wsl.conf` юниты и так включены, а задача гарантирует
подъём и покрывает случай ручного `wsl --shutdown`. Имя дистрибутива
отличается от `Ubuntu` — передай его параметром `-DistroName`.

## 7. Приёмка наряда (Сделано когда)

Три критерия — ровно из issue #21:

1. **Принудительная перезагрузка Windows** (Start → Перезагрузка при
   зажатом Shift — или выключение питания) → без единого ручного действия:

   ```bash
   systemctl is-active kamerton.service        # active
   systemctl is-active kamerton-backup.timer   # active
   schtasks /query /tn "Kamerton WSL boot" /v  # последний результат: 0x0
   ```

2. **Баннер `serve` печатает состояния exec- и env-гейтов**, и они
   совпадают с ожидаемым:

   ```bash
   journalctl -u kamerton.service -b | grep -E "\[serve\] route (exec|env):"
   ```

   Ожидаемо: `route exec: denied` (камертоновские маршруты не запускают
   внешние процессы) и env-строка с перечисленным allowlist — имена из
   `deploy/env.allowlist`: `KAMERTON_TG_TOKEN, KAMERTON_CHAT_ID,
   KAMERTON_DB, KAMERTON_TZ_OFFSET`. Если строка показывает `denied`
   по env — allowlist из env-файла не подхватился, проверь
   `EnvironmentFile` в `systemctl cat kamerton.service`.

3. **Бэкап восстанавливается в чистую директорию и читается `sqlite3`** —
   скрипт делает это сам при каждом запуске; прогони руками:

   ```bash
   sudo /data/kamerton/backup.sh
   # -> backup: ok -> ... (integrity ok, sqlite3 read-back ok, ...)
   ```

   Проверка читаемости сторонним клиентом — инвариант 6: база обязана
   читаться голым `sqlite3`, а не только приложением. Ретеншн: 8
   еженедельных копий, старые удаляются.

Результаты всех трёх проверок — в комментарий issue #21: это запись Шага 0
и приёмки наряда. Позже, когда Н1-06 смержит настоящий `app.mlog`,
повторный запуск `sudo bash deploy/install.sh` заменит заглушку на него
(тот же путь `/data/kamerton/app.mlog`, idемпотентно).
