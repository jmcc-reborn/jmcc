# Руководство разработчика по JustCode (.jc)

> [!NOTE]
> Английская версия руководства доступна в [GUIDE.md](GUIDE.md).

Добро пожаловать в полное практическое руководство по языку программирования **JustCode** (`.jc`) и компилятору **JMCC**.

JustCode — это статически типизированный язык высокого уровня с двуязычным синтаксисом (русский и английский), созданный для разработки игровых режимов и механик на серверах платформы JustMC (Minecraft с архитектурой DiamondFire). Компилятор `jmcc` оптимизирует код и транслирует его в исполняемые JSON-модули действий.

---

## 1. Установка и настройка окружения

### 1.1. Сборка компилятора и рантайма
Для сборки требуется компилятор Rust (канал `nightly`, редакция 2024):

```bash
# Клонирование и сборка в релизном режиме
cargo build --release
```

После сборки в папке `target/release/` появятся исполняемые файлы:
- `jmcc` — компилятор, менеджер пакетов, форматтер и тестовый раннер.
- `jmcmock` — виртуальная машина и мок-рантайм для отладки и запуска программ без входа в Minecraft.

Рекомендуется добавить путь к `target/release/` в системную переменную `PATH`.

### 1.2. Настройка Visual Studio Code
Для комфортной работы установите расширение из папки [`jmc-analyzer/`](jmc-analyzer):
- Подсветка синтаксиса для английских и русских ключевых слов.
- Интеграция с языковым сервером (LSP): автодополнение, всплывающие подсказки типов (hover), переход к определениям и мгновенная диагностика ошибок.

---

## 2. Первый проект: от нуля до запуска

### 2.1. Создание структуры проекта
Каждый проект JustCode содержит манифест `jmcc.toml` и исходный код. Создайте базовую структуру каталогов:

```text
my_game/
├── jmcc.toml
└── src/
    └── main.jc
```

Создайте файл `jmcc.toml`:
```toml
[package]
name = "my_game"
version = "0.1.0"
edition = 2026

[profile.dev]
opt_level = 1

[profile.release]
opt_level = 3
emit = ["json"]
```

### 2.2. Пишем первый код (`src/main.jc`)
Откройте `src/main.jc` и напишите обработчик входа игрока на сервер:

```jc
event<player_join> {
    var player_name = value::name<current>;
    player::message("Добро пожаловать на сервер, ${player_name}!");

    // Выдаём стартовый предмет
    player::give_items(item("compass"));
}
```

### 2.3. Компиляция проекта
Для сборки проекта перейдите в папку проекта и выполните команду:
```bash
jmcc compile
```
Компилятор проанализирует зависимости, проверит типы и сгенерирует файл `main.json` с оптимизированными инструкциями JustMC.

Для релизной сборки с максимальной оптимизацией:
```bash
jmcc compile --release
```

### 2.4. Проверка логики в `jmcmock`
Вам не нужно каждый раз перезагружать сервер Minecraft, чтобы проверить работу скрипта. Запустите его в эмуляторе:
```bash
jmcmock run main.json --event player_join
```
Рантайм сымитирует возникновение события `player_join`, выполнит действия и выведет лог отправленных сообщений и изменений инвентаря.

### 2.5. Загрузка на сервер JustMC
Скомпилированный модуль можно загрузить прямо на сервер:
```bash
# Загрузка через официальный способ JustMC API (по умолчанию)
jmcc compile -u

# Загрузка через вебхук Discord
jmcc compile -u webhook

# Загрузка через вебхук Discord с указанием своего вебхука
jmcc compile -u webhook --webhook-url "https://discord.com/api/webhooks/..."
```

Параметры загрузки и собственный URL вебхука можно также указать в `jmcc.toml`:
```toml
[upload]
target = "webhook" # или "official"
webhook_url = "https://discord.com/api/webhooks/..."
```

---

## 3. Двуязычный синтаксис

JustCode полностью равноправно поддерживает как классические английские, так и русские ключевые слова. Вы можете писать полностью на русском, полностью на английском или комбинировать их при необходимости.

#### Пример на английском языке:
```jc
class PlayerStats {
    var coins: number;

    function __init__(self: PlayerStats, coins: number) -> PlayerStats {
        self.coins = coins;
        return self;
    }

    function add_coins(self: PlayerStats, amount: number) {
        self.coins += amount;
    }
}

event<player_join> {
    var stats = PlayerStats(100);
    stats.add_coins(50);
    player::message("Total coins: ${stats.coins}");
}
```

#### Тот же самый код на русском языке:
```jc
класс ХарактеристикиИгрока {
    перем монеты: число;

    функция __конструктор__(сам: ХарактеристикиИгрока, монеты: число) -> ХарактеристикиИгрока {
        сам.монеты = монеты;
        вернуть сам;
    }

    функция добавить_монеты(сам: ХарактеристикиИгрока, количество: число) {
        сам.монеты += количество;
    }
}

событие<player_join> {
    перем статы = ХарактеристикиИгрока(100);
    статы.добавить_монеты(50);
    player::message("Всего монет: ${статы.монеты}");
}
```

---

## 4. Переменные и области видимости (Scopes)

В архитектуре JustMC физическая память разделена на четыре изолированных контекста. Выбор правильной области видимости критически важен для корректной работы игры:

| Область | Ключевое слово | Префикс | Время жизни и когда использовать |
|---|---|---|---|
| **Линейная** | `line var` | `` l`имя` `` | **Текущий обработчик.** Живёт только во время выполнения текущей цепочки действий. По умолчанию в издании 2026. Идеально для временных вычислений и счетчиков циклов. |
| **Локальная** | `local var` | `` l`имя` `` | **Текущий стек вызовов.** Сохраняется при вызове подфункций в рамках одного потока. |
| **Игровая** | `game var` | `` g`имя` `` | **Глобальное состояние игры.** Видна всем игрокам и процессам. Сбрасывается при перезагрузке мира. Подходит для таймеров матча, счёта команд, статуса лобби. |
| **Сохраняемая** | `save var` | `` s`имя` `` | **База данных сервера.** Персистентна. Сохраняется навсегда между перезапусками сервера. Идеально для профилей игроков, баланса, статистики, покупок. |

### Практические примеры объявления переменных:

```jc
// Линейная переменная (по умолчанию в 2026)
var current_step = 1;
line var temp_calc = 42;

// Глобальная переменная мира игры
game var match_state = "WAITING";
g`active_players` = 0;

// Персистентная переменная (сохраняется в базу данных сервера)
save var server_record = 1000;
s`player_balance` = 250;

// Константа времени компиляции (не занимает память на сервере)
inline var SPAWN_X = 100.5;
const MAX_PARTY_SIZE = 4;
```

### Переменные с экранированными именами (Backticks)
Если имя переменной содержит пробелы, символы плейсхолдеров или спецсимволы Minecraft, заключите его в грависы:
```jc
var `%player%_gems` = 15;
var `последний выбранный пункт` = "Меч";
```

---

## 5. Типы данных, строки и коллекции

### 5.1. Базовые типы
- `number` — вещественные числа (`10`, `3.14`, `-0.5`, `1e3`).
- `text` — строки текста.
- `boolean` — `true` / `false` (или `истина` / `ложь`).

### 5.2. Продвинутая работа со строками
JustCode поддерживает префиксы для точного указания формата отображения текста в Minecraft:

```jc
// 1. Legacy формат ( Minecraft цветовые коды & )
var t1 = l"&a[Успех] &fВы получили награду!";

// 2. MiniMessage формат (современные теги, градиенты, эффекты)
var t2 = m"<gradient:#ff5555:#55ff55>Новый уровень!</gradient>";
var t3 = m"<bold><gold>ВНИМАНИЕ:</gold></bold> <rainbow>Праздник на сервере!</rainbow>";

// 3. Plain формат (чистый плоский текст)
var t4 = p"Обычный текст без обработки символов &";

// 4. Строковая интерполяция
var user = "Alex";
var level = 42;
player::message("Игрок: ${user}, Уровень: ${level}, До следующего: ${100 - level}");
player::message("Краткая интерполяция: $user победил!");
```

### 5.3. Списки (`array`)
Динамические массивы поддерживают добавление, индексацию и срезы:

```jc
var inventory = ["меч", "лук", "стрелы", "зелье"];

// Чтение и запись по индексу (индексация с 0)
var first = inventory[0]; // "меч"
inventory[1] = "арбалет";

// Взятие среза [начало : конец]
var combat_gear = inventory[0:2]; // ["меч", "арбалет"]

// Проверка наличия через оператор 'in'
if "меч" in inventory {
    player::message("Оружие экипировано!");
}
```

### 5.4. Словари (`map`)
Ассоциативные массивы хранят данные по ключам:

```jc
var prices = {
    "алмаз": 100,
    "золото": 50,
    "железо": 10
};

// Чтение и изменение
var diamond_cost = prices["алмаз"];
prices["изумруд"] = 150;
```

### 5.5. Диапазоны (Ranges)
Диапазоны чисел создаются с помощью операторов `..` (полуинтервал) и `..=` (замкнутый интервал):

```jc
var r1 = 0..5;   // 0, 1, 2, 3, 4 (5 не включается)
var r2 = 0..=5;  // 0, 1, 2, 3, 4, 5 (5 включается)

// Методы диапазонов
var length = r1.len();               // 5
var has_three = r1.contains(3);      // true
var array_copy = r2.to_array();      // [0, 1, 2, 3, 4, 5]
var stepped = (0..10).step_by(2);    // шаг по 2 элемента
```

### 5.6. Предметы и NBT
Для задания кастомных компонентов предметов используется префикс `m{...}`:

```jc
var super_sword = item("netherite_sword", nbt = m{
    "minecraft:unbreakable": {},
    "minecraft:custom_name": "{\"text\":\"Клинок бури\",\"color\":\"aqua\"}",
    "CustomModelData": 1001
});

player::give_items(super_sword);
```

---

## 6. Управление потоком выполнения

### 6.1. Ветвления и тернарные операторы
```jc
if health <= 0 {
    player::message("Вы погибли!");
} elif health < 5 {
    player::message("Опасный уровень здоровья!");
} else {
    player::message("Состояние стабильное");
}

// Краткие тернарные формы:
var status = is_alive ? "В игре" : "Зритель";
var reward = 100 if is_vip else 25;
```

### 6.2. Циклы и метки
JustCode предоставляет мощный цикл `for .. in`, поддерживающий три сценария:

```jc
var items = ["яблоко", "хлеб", "стейк"];

// 1. Итерация по элементам
for food in items {
    player::message("Еда: ${food}");
}

// 2. Итерация по индексу и значению
for idx, food in items {
    player::message("${idx + 1}. ${food}");
}

// 3. Итерация по диапазону
for i in 1..=5 {
    player::message("Отсчёт: ${i}");
}

// 4. Итерация по словарю (ключ, значение)
var stats = {"Сила": 15, "Ловкость": 18};
for stat_name, stat_val in stats {
    player::message("${stat_name}: ${stat_val}");
}
```

#### Использование меток для выхода из вложенных циклов:
```jc
'search: for x in 0..10 {
    for y in 0..10 {
        if grid[x][y] == "target" {
            player::message("Найдено в (${x}, ${y})!");
            break 'search; // прерывает оба цикла
        }
    }
}
```

### 6.3. Сопоставление с образцом: `match`
Конструкция `match` делает обработку множественных условий наглядной и безопасной:

```jc
function handle_command(cmd: text) {
    match cmd {
        "start" | "play" => {
            player::message("Игра начинается!");
        }
        "help" => {
            player::message("Список доступных команд: start, help, quit");
        }
        _ if cmd.starts_with("warp_") => {
            player::message("Телепортация на локацию: ${cmd}");
        }
        _ => {
            player::message("Неизвестная команда. Введите 'help'.");
        }
    }
}
```

### 6.4. Обработка ошибок: `try` / `catch` / `throw`
```jc
try {
    if divisor == 0 {
        throw ERROR "Попытка деления на ноль";
    }
    var result = total / divisor;
} catch (error) {
    player::message("Ошибка вычисления: ${error}");
}
```

---

## 7. Функции, методы и лямбда-выражения

### 7.1. Функции и параметры
```jc
// Параметры по умолчанию и тип возврата
function calculate_exp(base: number, multiplier: number = 1.5) -> number {
    return base * multiplier;
}

// Передача по ссылке: модифицирует оригинальную переменную
function give_bonus(ref balance: number, bonus: number) {
    balance += bonus;
}

var my_coins = 100;
give_bonus(my_coins, 50);
// my_coins теперь равен 150!
```

### 7.2. Инлайн-функции (`inline function`)
Если функция вызывается часто, добавьте модификатор `inline`. Компилятор подставит её тело прямо в место вызова, устранив создание подпрограммы:

```jc
inline function clamp(val: number, min_val: number, max_val: number) -> number {
    if val < min_val { return min_val; }
    if val > max_val { return max_val; }
    return val;
}
```

> [!NOTE]
> Функции стандартной библиотеки `max` и `min` принимают список чисел (например, `max([3, 9])`), либо вызываются как метод числа (например, `3.max([9])`).

### 7.3. Лямбды и функциональное программирование
Лямбда-выражения позволяют передавать логику как значения:

```jc
var square = (x: number) => x * x;
var sum = (a: number, b: number) => a + b;

// Многострочная лямбда с блоком
var process_score = (score: number) => {
    var bonus = score > 100 ? 50 : 10;
    return score + bonus;
};

// Передача лямбды в функцию
function apply_math(val: number, operation: Fn<number, number>) -> number {
    return operation(val);
}

var res = apply_math(10, (x: number) => x * 3); // 30
```

---

## 8. Объектно-ориентированное программирование

### 8.1. Классы и конструкторы
Классы позволяют объединять данные и поведение в удобные структуры:

```jc
class Vector2 {
    var x: number;
    var y: number;

    // Конструктор экземпляра
    function __init__(self: Vector2, x: number, y: number) -> Vector2 {
        self.x = x;
        self.y = y;
        return self;
    }

    // Обычный метод
    function length_sq(self: Vector2) -> number {
        return self.x * self.x + self.y * self.y;
    }
}
```

### 8.2. Интерфейсы (`interface`)
Интерфейсы задают обязательный контракт для классов:

```jc
interface Damageable {
    function take_damage(self: Damageable, amount: number);
}

interface Healer {
    function heal(self: Healer, amount: number);
}

// Множественное наследование интерфейсов
interface Combatant extends Damageable, Healer {}

class Boss implements Combatant {
    var health: number;

    function __init__(self: Boss, health: number) -> Boss {
        self.health = health;
        return self;
    }

    function take_damage(self: Boss, amount: number) {
        self.health -= amount;
        player::message("Босс получил урон! Осталось: ${self.health}");
    }

    function heal(self: Boss, amount: number) {
        self.health += amount;
    }
}
```

### 8.3. Перегрузка операторов (Dunder-методы)
Хотите складывать свои классы через `+` или обращаться по индексу через `[]`? Просто реализуйте соответствующие методы:

```jc
class Point {
    var x: number;
    var y: number;

    function __init__(self: Point, x: number, y: number) -> Point {
        self.x = x;
        self.y = y;
        return self;
    }

    // Перегрузка оператора + (__add__)
    function __add__(self: Point, other: Point) -> Point {
        return Point(self.x + other.x, self.y + other.y);
    }

    // Красивое строковое представление при интерполяции (__str__)
    function __str__(self: Point) -> text {
        return "Point(${self.x}, ${self.y})";
    }
}

event<player_join> {
    var p1 = Point(10, 20);
    var p2 = Point(5, 15);
    var p3 = p1 + p2; // вызывает p1.__add__(p2)
    player::message("Результат: ${p3}"); // Point(15, 35)
}
```

---

## 9. Программирование для JustMC (Minecraft)

### 9.1. Игровые события
Каждый скрипт JustCode начинается с обработчиков событий:
- `event<player_join>` — вход игрока.
- `event<player_quit>` — выход игрока.
- `event<player_damage_player>` — нанесение урона другому игроку.
- `event<player_right_click>` — правый клик мышью.
- `event<player_chat>` — сообщение в чат.

### 9.2. Вызовы действий и селекторы
Действия вызываются через форму `категория::действие<селектор>(аргументы)`. Селектор определяет, кто именно является целью действия:

```jc
event<player_damage_player> {
    // Жертве отправляем сообщение
    player::message<victim>("Вам нанёс урон игрок: " + value::name<damager>);

    // Атакующему проигрываем победный звук
    player::play_sound<damager>(sound("entity.experience_orb.pickup"));

    // Оповещаем весь сервер
    player::send_action_bar<all_players>("Идёт активный бой!");
}
```

### 9.3. Чтение игровых величин (`value::...`)
Игровые величины возвращают состояние мира или игрока:
```jc
var my_health = value::current_health<current>;
var my_coords = value::location<current>;
var online_count = value::player_count;
```

### 9.4. Параллельные процессы (`process`)
Процессы — это асинхронные задачи, работающие в фоновом режиме. Внутри них можно использовать паузы (`code::wait`):

```jc
process match_countdown(seconds: number) {
    while seconds > 0 {
        player::send_title<all_players>(
            m"<gold>${seconds}</gold>", 
            subtitle = "До начала матча", 
            fade_in = 0, stay = 20, fade_out = 0
        );
        code::wait(1, time_unit = "SECONDS");
        seconds -= 1;
    }

    player::send_title<all_players>(m"<green>СТАРТ!</green>");
    game::is_match_running = true;
}

event<player_join> {
    // Запуск процесса в фоновом потоке
    match_countdown(5);
}
```

---

## 10. Модули, пакеты и тестирование

### 10.1. Организация модулей (Издание 2026)
Разбивайте крупные проекты на несколько файлов:

```jc
// src/utils/math.jc
export function power_of_two(n: number) -> number {
    return 1 << n;
}

function secret_internal() {
    // без 'export' функция недоступна из других файлов
}
```

Импортируйте функции в главном файле:
```jc
// src/main.jc
import "./utils/math.jc";

event<player_join> {
    var val = power_of_two(4); // 16
}
```

### 10.2. Встроенный тестовый раннер (`jmcc test`)
Пишите тесты прямо в коде, пометив их декоратором `@test`:

```jc
// tests/math_test.jc
import "./src/utils/math.jc";

@test
function test_power() {
    var res = power_of_two(3);
    if res != 8 {
        throw ERROR "Неверный результат возведения в степень";
    }
}

@test
@should_panic(expected = "Деление на ноль")
function test_zero_div() {
    var bad = 10 / 0;
}
```

Запустите тесты одной командой:
```bash
jmcc test
```
Компилятор прогонит все тесты в виртуальной среде `jmcmock` и покажет подробный отчёт о результатах.

---

## 11. Практические рецепты разработки (Cookbook)

### Рецепт 1: Приветствие новичка с сохранением первого входа
```jc
event<player_join> {
    var player_name = value::name<current>;

    // Проверяем персистентную переменную в базе сервера
    if not s`has_joined_${player_name}` {
        s`has_joined_${player_name}` = true;

        player::message<all_players>(m"<yellow>Впервые на сервере: <bold>${player_name}</bold>!</yellow>");
        player::give_items(item("stone_sword"), item("bread", count = 16));
        player::teleport(location(0.5, 65, 0.5));
    } else {
        player::message("С возвращением, ${player_name}!");
    }
}
```

### Рецепт 2: Система баланса и покупок в магазине
```jc
function buy_item(cost: number, item_id: text) {
    var name = value::name<current>;
    var current_money = s`balance_${name}` as number;

    if current_money >= cost {
        s`balance_${name}` = current_money - cost;
        player::give_items(item(item_id));
        player::message(m"<green>Вы успешно купили ${item_id} за ${cost} монет!</green>");
        player::play_sound(sound("entity.player.levelup"));
    } else {
        var needed = cost - current_money;
        player::message(m"<red>Недостаточно средств! Не хватает ещё ${needed} монет.</red>");
        player::play_sound(sound("entity.villager.no"));
    }
}
```

### Рецепт 3: Интерактивный таймер регенерации здоровья
```jc
process health_regen_loop() {
    while true {
        code::wait(3, time_unit = "SECONDS");

        for target in value::all_players {
            var current_hp = value::current_health<target>;
            var max_hp = value::max_health<target>;

            if current_hp < max_hp {
                player::set_health<target>(current_hp + 1);
                player::send_action_bar<target>(m"<green>+1 Здоровье (Регенерация)</green>");
            }
        }
    }
}

event<world_start> {
    health_regen_loop();
}
```

---

## 12. Шпаргалка по синтаксису (Cheat Sheet)

### Ключевые слова
```jc
// Объявление переменных:
var x = 10;            // line var (2026)
game var score = 0;    // глобальная сессии
save var bank = 100;   // персистентная БД
inline var PI = 3.14;  // константа компилятора

// Функции и условия:
function calc(a: number, b: number) -> number { return a + b; }
var res = cond ? "да" : "нет";

// Циклы:
for item in list { ... }
for idx, item in list { ... }
for key, val in map { ... }
for i in 0..10 { ... }
while is_active { ... }

// Сопоставление с образцом:
match value {
    1 | 2 => "мало",
    _ if value > 10 => "много",
    _ => "другое"
}
```

### Команды CLI
- `jmcc new <имя>` — создать новый пакет JustCode (`--bin` или `--lib`).
- `jmcc compile` — собрать проект в JSON.
- `jmcc compile --release` — релизная сборка с максимальными оптимизациями.
- `jmcc compile -u` — собрать и мгновенно загрузить модуль на сервер Minecraft.
- `jmcc test` — запустить все модульные тесты.
- `jmcc format .` — отформатировать все файлы `.jc` в проекте.
- `jmcmock run out.json --event player_join` — протестировать скомпилированный файл в виртуальном рантайме.
