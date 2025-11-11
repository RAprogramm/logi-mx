# Архитектура драйвера Logitech MX Master 3S

## 📋 Оглавление

1. [Общая архитектура](#общая-архитектура)
2. [HID++ 2.0 Протокол](#hidpp-20-протокол)
3. [Структура пакетов](#структура-пакетов)
4. [Feature Discovery](#feature-discovery)
5. [Работа с устройством](#работа-с-устройством)
6. [Обработка ошибок](#обработка-ошибок)
7. [Примеры команд](#примеры-команд)

---

## Общая архитектура

```
┌─────────────────────────────────────────────────────────────┐
│                         Пользователь                        │
└────────────────────┬───────────────────┬────────────────────┘
                     │                   │
         ┌───────────▼──────────┐   ┌────▼──────────┐
         │   CLI (logi-mx)      │   │   Daemon      │
         │  - info              │   │  - udev watch │
         │  - battery           │   │  - auto-apply │
         │  - set dpi/smart...  │   │               │
         └───────────┬──────────┘   └────┬──────────┘
                     │                   │
         ┌───────────▼───────────────────▼──────────┐
         │     Driver Library (logi-mx-driver)      │
         │  ┌────────────────────────────────────┐  │
         │  │  MX Master 3S Device               │  │
         │  │  - get_dpi(), set_dpi()            │  │
         │  │  - get_smartshift(), set_smart...  │  │
         │  │  - get_battery()                   │  │
         │  └──────────────┬─────────────────────┘  │
         │                 │                         │
         │  ┌──────────────▼─────────────────────┐  │
         │  │  HID++ Device Layer                │  │
         │  │  - send_command()                  │  │
         │  │  - get_feature_index()             │  │
         │  │  - retry logic, error handling     │  │
         │  └──────────────┬─────────────────────┘  │
         │                 │                         │
         │  ┌──────────────▼─────────────────────┐  │
         │  │  HID++ Packet Layer                │  │
         │  │  - Short/Long packet parsing       │  │
         │  │  - to_bytes() / from_bytes()       │  │
         │  └────────────────────────────────────┘  │
         └───────────────────┬──────────────────────┘
                             │
         ┌───────────────────▼──────────────────────┐
         │   hidapi (libhidapi-hidraw)              │
         │   - read/write to HID devices            │
         └───────────────────┬──────────────────────┘
                             │
         ┌───────────────────▼──────────────────────┐
         │   Linux Kernel: /dev/hidraw2             │
         │   - Interface 2 (HID++)                  │
         └───────────────────┬──────────────────────┘
                             │
                    ┌────────▼─────────┐
                    │  Logi Bolt USB   │
                    │  Receiver        │
                    │  VID:046d        │
                    │  PID:c548        │
                    └────────┬─────────┘
                             │
                             │ RF 2.4GHz
                             │
                    ┌────────▼─────────┐
                    │  MX Master 3S    │
                    │  For Business    │
                    └──────────────────┘
```

---

## HID++ 2.0 Протокол

### Что такое HID++?

HID++ (HID Plus Plus) - проприетарный протокол Logitech для расширенной коммуникации с устройствами.
Работает поверх стандартного USB HID.

### Особенности протокола:

- **Feature-based**: Каждая функция имеет уникальный ID (например, 0x2201 = Adjustable DPI)
- **Dynamic discovery**: Индексы функций определяются динамически через Root Feature (0x0000)
- **Two packet sizes**: Short (7 bytes) и Long (20 bytes)
- **Software ID**: Идентификатор приложения (мы используем 0x05)
- **Device index**: Номер устройства в receiver (0xFF для проводных, 1-6 для wireless)

---

## Структура пакетов

### Short Packet (7 bytes)

```
┌──────┬────────┬───────┬─────────┬──────────┬──────────────┐
│ 0x10 │ DevIdx │ FeatIdx │ FuncID │ SoftID │ Parameters │
│  1B  │   1B   │   1B    │   1B   │   1B   │    3B      │
└──────┴────────┴─────────┴─────────┴────────┴──────────────┘

Пример: Ping устройства с index 2
[0x10, 0x02, 0x00, 0x01, 0x05, 0x00, 0x00, 0x00]
  │     │     │     │     │     └───────────┘
  │     │     │     │     │          └─ Parameters
  │     │     │     │     └─ Software ID
  │     │     │     └─ Function ID: Ping (1)
  │     │     └─ Feature Index: Root (0)
  │     └─ Device Index: 2
  └─ Report ID: Short packet
```

### Long Packet (20 bytes)

```
┌──────┬────────┬───────┬─────────┬──────────┬──────────────┐
│ 0x11 │ DevIdx │ FeatIdx │ FuncID │ SoftID │ Parameters │
│  1B  │   1B   │   1B    │   1B   │   1B   │   16B      │
└──────┴────────┴─────────┴─────────┴────────┴──────────────┘

Пример: Get Device Name с offset 0
[0x11, 0x02, 0x03, 0x00, 0x05, 0x00, 0x00, 0x00, ...]
  │     │     │     │     │     │    └────────────┘
  │     │     │     │     │     │          └─ 15 байт параметров
  │     │     │     │     │     └─ Offset: 0
  │     │     │     │     └─ Software ID
  │     │     │     └─ Function: GetDeviceName (0)
  │     │     └─ Feature Index: DeviceName (3)
  │     └─ Device Index: 2
  └─ Report ID: Long packet
```

### Packet Error Response

Устройство возвращает ошибку в специальном формате:

```
[0x8F, DevIdx, FeatureIdx, FuncID, SoftID, ErrorCode]
  │
  └─ 0x8F = Error marker
```

Коды ошибок:
- `0x01` - Invalid SubID (неверный function ID)
- `0x02` - Invalid Address
- `0x03` - Invalid Value
- `0x05` - Connection Failed
- `0x06` - Too Many Devices
- `0x07` - Already Exists
- `0x08` - Busy (устройство занято)
- `0x09` - Unknown Device
- `0x0A` - Resource Error
- `0x0B` - Request Unavailable
- `0x0C` - Unsupported Parameter
- `0x0D` - Wrong PIN Code

---

## Feature Discovery

HID++ использует динамическое обнаружение функций через **Root Feature (0x0000)**.

### Алгоритм:

1. **Отправляем запрос GetFeature:**
   ```
   Feature Index: 0 (Root)
   Function ID: 0 (GetFeature)
   Parameters: [0x22, 0x01, 0x00]  // Feature ID 0x2201 (DPI)
   ```

2. **Получаем ответ:**
   ```
   Parameters: [0x0D, 0x00, 0x02]
                 │     │     └─ Version: 2
                 │     └─ Type flags
                 └─ Feature Index: 13
   ```

3. **Кешируем результат:**
   ```rust
   feature_cache.insert(0x2201, 13);
   ```

### Код реализации:

```rust
pub fn get_feature_index(&mut self, feature_id: u16) -> Result<u8> {
    // Проверяем кеш
    if let Some(&index) = self.feature_cache.get(&feature_id) {
        return Ok(index);
    }

    // Запрашиваем у устройства
    let params = [(feature_id >> 8) as u8, (feature_id & 0xFF) as u8, 0x00];
    let response = self.send_command(ROOT_INDEX, RootFunction::GetFeature as u8, &params)?;

    // Извлекаем индекс из ответа
    let index = match response {
        HidppPacket::Short(p) => p.parameters[0],
        HidppPacket::Long(p) => p.parameters[0]
    };

    if index == 0 {
        return Err(DeviceErrorKind::UnsupportedFeature.into());
    }

    // Сохраняем в кеш
    self.feature_cache.insert(feature_id, index);
    debug!("Feature {:04x} mapped to index {}", feature_id, index);

    Ok(index)
}
```

---

## Работа с устройством

### Открытие устройства

```rust
// driver/src/hidpp/device.rs:45
pub fn open_vid_pid(vendor_id: u16, product_id: u16, device_index: u8) -> Result<Self> {
    let api = HidApi::new()?;

    // Ищем interface 2 (HID++ interface)
    let mut target_path = None;
    for device_info in api.device_list() {
        if device_info.vendor_id() == vendor_id
           && device_info.product_id() == product_id {
            if device_info.interface_number() == 2 || device_info.interface_number() == -1 {
                target_path = Some(device_info.path().to_owned());
                break;
            }
        }
    }

    let path = target_path.ok_or(...)?;
    let device = api.open_path(&path)?;

    Ok(Self {
        device,
        device_index,
        feature_cache: HashMap::new(),
        software_id: 0x05
    })
}
```

### Отправка команды с автоповтором

```rust
pub fn send_command(
    &mut self,
    feature_index: u8,
    function_id: u8,
    params: &[u8]
) -> Result<HidppPacket> {
    // Выбираем тип пакета
    let packet = if params.len() <= 3 {
        HidppPacket::new_short(self.device_index, feature_index, function_id,
                               self.software_id, parameters)
    } else if params.len() <= 16 {
        HidppPacket::new_long(self.device_index, feature_index, function_id,
                              self.software_id, parameters)
    } else {
        return Err(AppError::bad_request("Parameters too long"));
    };

    // Пробуем 3 раза с задержкой
    for attempt in 0..RETRY_COUNT {
        match self.send_packet_with_response(&packet) {
            Ok(response) => {
                if response.is_error() {
                    if let Some(error_code) = response.get_error_code() {
                        if error_code == ERROR_BUSY && attempt < RETRY_COUNT - 1 {
                            warn!("Device busy, retrying... (attempt {})", attempt + 1);
                            std::thread::sleep(Duration::from_millis(50));
                            continue;
                        }
                        return Err(self.map_hidpp_error(error_code));
                    }
                }
                return Ok(response);
            }
            Err(e) if attempt < RETRY_COUNT - 1 => {
                warn!("Command failed, retrying... (attempt {})", attempt + 1);
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e)
        }
    }

    Err(DeviceErrorKind::CommandFailed.into())
}
```

---

## Обработка ошибок

### Иерархия ошибок

```rust
pub enum DeviceErrorKind {
    NotFound,              // Устройство не найдено
    ConnectionFailed,      // Ошибка подключения
    InvalidResponse,       // Неверный ответ от устройства
    UnsupportedFeature,    // Функция не поддерживается
    CommandFailed,         // Команда не выполнена
    Timeout               // Таймаут коммуникации
}
```

### Преобразование HID++ ошибок

```rust
fn map_hidpp_error(&self, error_code: u8) -> AppError {
    match error_code {
        ERROR_INVALID_SUBID => AppError::bad_request("Invalid function ID"),
        ERROR_INVALID_ADDRESS => AppError::bad_request("Invalid address"),
        ERROR_INVALID_VALUE => AppError::bad_request("Invalid value"),
        ERROR_CONNECT_FAIL => AppError::internal("Connection failed"),
        ERROR_TOO_MANY_DEVICES => AppError::conflict("Too many devices"),
        ERROR_ALREADY_EXISTS => AppError::conflict("Already exists"),
        ERROR_BUSY => AppError::internal("Device busy"),
        ERROR_UNKNOWN_DEVICE => AppError::not_found("Unknown device"),
        ERROR_RESOURCE_ERROR => AppError::internal("Resource error"),
        ERROR_REQUEST_UNAVAILABLE => AppError::bad_request("Request unavailable"),
        ERROR_UNSUPPORTED_PARAM => AppError::bad_request("Unsupported parameter"),
        ERROR_WRONG_PIN_CODE => AppError::unauthorized("Wrong PIN code"),
        _ => AppError::internal("Unknown HID++ error")
            .with_field(field::u64("error_code", error_code as u64))
    }
}
```

---

## Примеры команд

### 1. Установка DPI

```
┌──────────────────────────────────────────────────────────────┐
│ Command: Set DPI to 1800 (0x0708)                           │
└──────────────────────────────────────────────────────────────┘

Step 1: Get feature index for 0x2201 (Adjustable DPI)
  → [0x10, 0x02, 0x00, 0x00, 0x05, 0x22, 0x01, 0x00]
  ← [0x11, 0x02, 0x00, 0x00, 0x05, 0x0D, 0x00, 0x02, ...]
     Feature index = 13 (0x0D)

Step 2: Set DPI value
  → [0x10, 0x02, 0x0D, 0x03, 0x05, 0x00, 0x07, 0x08]
                  │     │              │     └───┴── DPI: 1800
                  │     │              └── Sensor ID: 0
                  │     └── Function: SetSensorDpi (3)
                  └── Feature Index: 13
  ← ACK or error response
```

**Код:**

```rust
fn set_dpi(&mut self, dpi: u16) -> Result<()> {
    let feature_index = self.hidpp.get_feature_index(FEATURE_ADJUSTABLE_DPI)?;

    // Параметры: [sensor_id, dpi_hi, dpi_lo]
    let params = [0x00, (dpi >> 8) as u8, (dpi & 0xFF) as u8];

    self.hidpp.send_command(
        feature_index,
        DpiFunction::SetSensorDpi as u8,
        &params
    )?;

    Ok(())
}
```

### 2. SmartShift (Feature 0x2110)

SmartShift управляет моторизованной трещоткой колеса прокрутки.

```
┌──────────────────────────────────────────────────────────────┐
│ Command: Enable SmartShift with threshold 30                │
└──────────────────────────────────────────────────────────────┘

Step 1: Get feature index for 0x2110
  → [0x10, 0x02, 0x00, 0x00, 0x05, 0x21, 0x10, 0x00]
  ← [0x11, 0x02, 0x00, 0x00, 0x05, 0x0E, 0x00, 0x01, ...]
     Feature index = 14 (0x0E)

Step 2: Set ratchet control mode
  → [0x10, 0x02, 0x0E, 0x01, 0x05, 0x02, 0x1E, 0x00]
                  │     │              │     │     └── Default: no change
                  │     │              │     └── Threshold: 30 (0x1E)
                  │     │              └── Wheel Mode: 2 (Ratchet)
                  │     └── Function: SetRatchetControlMode (1)
                  └── Feature Index: 14
```

**Параметры:**

- **Byte 0 (wheelMode):**
  - `0x00` = No change
  - `0x01` = Freespin (свободное вращение)
  - `0x02` = Ratchet (с трещоткой)

- **Byte 1 (autoDisengage):**
  - `0x00` = No change
  - `0x01-0xFE` = Threshold (скорость для автопереключения в четвертьоборотах/сек)
  - `0xFF` = Always engaged (всегда включено, без автопереключения)

- **Byte 2 (autoDisengageDefault):**
  - `0x00` = No change
  - `0x01-0xFE` = Default threshold
  - `0xFF` = Always engaged

**Код:**

```rust
fn set_smartshift(&mut self, config: SmartShiftConfig) -> Result<()> {
    let feature_index = self.hidpp.get_feature_index(FEATURE_SMART_SHIFT)?;

    let wheel_mode = 0x02;  // Always ratchet mode
    let auto_disengage = if config.enabled && config.threshold > 0 {
        config.threshold
    } else {
        0xFF  // Disabled (always engaged, no auto-switch)
    };
    let auto_disengage_default = 0x00;  // No change to persistent default

    let params = [wheel_mode, auto_disengage, auto_disengage_default];

    self.hidpp.send_command(
        feature_index,
        SmartShiftFunction::SetRatchetControlMode as u8,
        &params
    )?;

    Ok(())
}
```

### 3. Батарея (Feature 0x1004/0x1000)

```
┌──────────────────────────────────────────────────────────────┐
│ Command: Get Battery Status                                 │
└──────────────────────────────────────────────────────────────┘

Step 1: Try Unified Battery (0x1004)
  → [0x10, 0x02, 0x00, 0x00, 0x05, 0x10, 0x04, 0x00]
  ← [0x11, 0x02, 0x00, 0x00, 0x05, 0x0C, 0x00, 0x00, ...]
     Feature index = 12

Step 2: Get battery status
  → [0x10, 0x02, 0x0C, 0x00, 0x05, 0x00, 0x00, 0x00]
  ← [0x11, 0x02, 0x0C, 0x00, 0x05, 0x0F, 0x00, 0x00, ...]
                                    │     └── Status: 0=Discharging
                                    └── Level: 15%
```

**Status Codes:**

- `0` = Discharging
- `1` = Charging
- `2` = Full

**Код:**

```rust
fn get_battery_unified(&mut self) -> Result<BatteryInfo> {
    let feature_index = self.hidpp.get_feature_index(FEATURE_UNIFIED_BATTERY)?;

    let response = self.hidpp.send_command(
        feature_index,
        BatteryFunction::GetStatus as u8,
        &[]
    )?;

    let (level, status_byte) = match response {
        HidppPacket::Short(p) => (p.parameters[0], p.parameters[1]),
        HidppPacket::Long(p) => (p.parameters[0], p.parameters[1])
    };

    let status = match status_byte {
        0 => BatteryStatus::Discharging,
        1 => BatteryStatus::Charging,
        2 => BatteryStatus::Full,
        _ => BatteryStatus::Unknown
    };

    Ok(BatteryInfo { level, status })
}
```

---

## Таблица Feature ID

| Feature ID | Название              | Функции                                    |
|------------|-----------------------|--------------------------------------------|
| `0x0000`   | Root                  | 0:GetFeature, 1:Ping                       |
| `0x0005`   | Device Name           | 0:GetDeviceName                            |
| `0x1000`   | Battery Status        | 0:GetStatus, 1:GetCapability               |
| `0x1004`   | Unified Battery       | 0:GetStatus, 1:GetCapability               |
| `0x2110`   | SmartShift            | 0:GetRatchetControlMode, 1:SetRatchet...   |
| `0x2121`   | Hi-Res Wheel          | 0:GetCapabilities, 1:GetMode, 2:SetMode    |
| `0x2201`   | Adjustable DPI        | 0:GetSensorCount, 2:GetSensorDpi, 3:Set... |
| `0x1B04`   | Reprog Controls v4    | 0:GetControlCount, 1:GetControlInfo, ...   |

---

## Производительность

**Время выполнения команд (на реальном устройстве):**

```bash
$ time ./target/release/logi-mx info > /dev/null
real    0m0.965s
user    0m0.007s
sys     0m0.007s

$ time ./target/release/logi-mx set dpi 2000
real    0m0.889s
user    0m0.006s
sys     0m0.006s
```

**Задержки:**
- Открытие устройства: ~20ms
- Feature discovery (первый раз): ~20ms на feature
- Feature discovery (cached): <1ms
- Отправка команды: ~15-25ms
- Retry при ошибке: 50ms задержка

---

## Устройства USB интерфейсов

Logi Bolt Receiver (046d:c548) имеет 3 интерфейса:

```
Interface 0: Mouse HID (standard)
  - /dev/hidraw0
  - Обычные mouse events (движение, клики)

Interface 1: Keyboard HID (standard)
  - /dev/hidraw1
  - Keyboard events от клавиатур в receiver

Interface 2: HID++ Protocol
  - /dev/hidraw2  ← Используем этот!
  - Расширенная коммуникация (DPI, SmartShift, etc.)
```

**Важно:** Драйвер автоматически находит interface 2 по `interface_number() == 2`.

---

## Zero-Cost Abstractions

Драйвер использует современные паттерны Rust без потери производительности:

- **Enum dispatch вместо trait objects**: `HidppPacket::Short/Long` без vtable
- **inline-always** для горячих путей: `to_bytes()`, `from_bytes()`
- **Stack allocation**: Буферы фиксированного размера без heap
- **Feature cache**: HashMap для O(1) lookup после первого запроса
- **LTO + single codegen unit**: Агрессивная оптимизация в release

---

## Troubleshooting

### Устройство не открывается

```bash
# Проверить USB устройства
lsusb | grep Logitech

# Проверить hidraw устройства
ls -la /dev/hidraw*

# Проверить udev правила
cat /etc/udev/rules.d/90-logi-mx.rules

# Перезагрузить udev
sudo udevadm control --reload-rules
sudo udevadm trigger
```

### Команды не работают

```bash
# Включить trace логирование
RUST_LOG=trace ./target/release/logi-mx info

# Проверить интерфейс
udevadm info /dev/hidraw2 | grep INTERFACE
```

### Батарея показывает Unknown

Некоторые старые мыши используют legacy battery protocol (feature 0x1000 вместо 0x1004).
Драйвер автоматически пробует оба варианта через `get_battery_unified().or_else(|_| get_battery_legacy())`.

---

## Дополнительные ресурсы

- **HID++ Спецификации**: https://lekensteyn.nl/logitech-unifying.html
- **Solaar (reference implementation)**: https://github.com/pwr-Solaar/Solaar
- **libratbag**: https://github.com/libratbag/libratbag
- **HID Usage Tables**: https://usb.org/hid

---

**Дата**: 2025-11-11
**Версия драйвера**: 0.1.0
**Rust Edition**: 2024
