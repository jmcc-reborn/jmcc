# JustCode (.jc) Developer Guide

> [!NOTE]
> The Russian version of this guide is available at [GUIDE_RU.md](GUIDE_RU.md).

Welcome to the comprehensive practical guide to the **JustCode** (`.jc`) programming language and the **JMCC** compiler.

JustCode is a high-level, statically typed language featuring bilingual syntax (English and Russian), purpose-built for developing games and custom mechanics for JustMC Minecraft servers (DiamondFire architecture). The `jmcc` compiler optimizes code and lowers it into executable JSON action modules.

---

## 1. Environment Setup & Installation

### 1.1. Building the Compiler and Runtime
Building requires the Rust compiler (`nightly` channel, edition 2024):

```bash
# Clone and build in release mode
cargo build --release
```

After building, the binaries will be located in `target/release/`:
- `jmcc` — Compiler, package manager, code formatter, and test runner.
- `jmcmock` — Virtual machine and mock runtime for testing and running programs without launching Minecraft.

It is recommended to add `target/release/` to your system `PATH`.

### 1.2. Visual Studio Code Configuration
Install the extension from [`jmc-analyzer/`](jmc-analyzer):
- Full syntax highlighting for English and Russian keywords.
- Language Server (LSP) integration: autocompletion, type hover tips, go to definition, and instant diagnostics.

---

## 2. Your First Project: Zero to Running

### 2.1. Project Directory Layout
Every JustCode project contains a `jmcc.toml` manifest and source files. Create the basic directory layout:

```text
my_game/
├── jmcc.toml
└── src/
    └── main.jc
```

Create `jmcc.toml`:
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

### 2.2. Writing Your First Code (`src/main.jc`)
Open `src/main.jc` and define a player join event handler:

```jc
event<player_join> {
    var player_name = value::name<current>;
    player::message("Welcome to the server, ${player_name}!");

    // Give starter item
    player::give_items(item("compass"));
}
```

### 2.3. Compiling the Project
Navigate to your project directory and run:
```bash
jmcc compile
```
The compiler analyzes dependencies, type-checks the code, and emits `main.json` containing optimized JustMC instructions.

For a release build with maximal optimizations:
```bash
jmcc compile --release
```

### 2.4. Verifying Logic with `jmcmock`
You don't need to restart a Minecraft server to test your script. Run it in the emulator:
```bash
jmcmock run main.json --event player_join
```
The runtime simulates the `player_join` event, executes actions, and prints logs of sent messages and inventory mutations.

### 2.5. Uploading to JustMC Server
You can upload compiled modules directly to the server:
```bash
# Upload via official JustMC API (default)
jmcc compile -u

# Upload via Discord webhook
jmcc compile -u webhook

# Upload via Discord webhook using a custom webhook URL
jmcc compile -u webhook --webhook-url "https://discord.com/api/webhooks/..."
```

Upload settings and custom webhook URLs can also be configured in `jmcc.toml`:
```toml
[upload]
target = "webhook" # or "official"
webhook_url = "https://discord.com/api/webhooks/..."
```

---

## 3. Bilingual Syntax

JustCode provides full first-class support for both English and Russian keywords. You can write in pure English, pure Russian, or mix them seamlessly when needed.

#### English version:
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

#### Equivalent Russian version:
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

## 4. Variables and Storage Scopes

In the JustMC architecture, physical memory is partitioned into four isolated contexts. Choosing the correct scope is critical:

| Scope | Keyword | Prefix | Lifetime and Usage |
|---|---|---|---|
| **Line** | `line var` | `` l`name` `` | **Current handler line.** Survives only during execution of the current action line. Default in Edition 2026. Ideal for temporary calculations and loop counters. |
| **Local** | `local var` | `` l`name` `` | **Thread call stack.** Preserved across sub-function calls within the same execution thread. |
| **Game** | `game var` | `` g`name` `` | **Global game state.** Visible to all players and processes. Cleared on world reload. Used for match timers, team scores, lobby status. |
| **Save** | `save var` | `` s`name` `` | **Persistent server database.** Stored permanently across server restarts. Ideal for player profiles, wallet balances, stats, purchases. |

### Practical Examples of Variable Declarations:

```jc
// Line variable (default in 2026)
var current_step = 1;
line var temp_calc = 42;

// Global game world variable
game var match_state = "WAITING";
g`active_players` = 0;

// Persistent variable (saved in server database)
save var server_record = 1000;
s`player_balance` = 250;

// Compile-time constant (zero memory allocation on server)
inline var SPAWN_X = 100.5;
const MAX_PARTY_SIZE = 4;
```

### Escaped Variable Identifiers (Backticks)
If an identifier contains spaces, placeholders, or Minecraft color formatting, wrap it in backticks:
```jc
var `%player%_gems` = 15;
var `last selected item` = "Sword";
```

---

## 5. Data Types, Strings, and Collections

### 5.1. Basic Types
- `number` — 64-bit floating-point numbers (`10`, `3.14`, `-0.5`, `1e3`).
- `text` — Text strings.
- `boolean` — `true` / `false` (or `истина` / `ложь`).

### 5.2. Advanced String Formatting
JustCode supports prefixes to control text rendering on Minecraft clients:

```jc
// 1. Legacy format (Minecraft color codes &)
var t1 = l"&a[Success] &fReward received!";

// 2. MiniMessage format (modern tags, gradients, animations)
var t2 = m"<gradient:#ff5555:#55ff55>Level Up!</gradient>";
var t3 = m"<bold><gold>WARNING:</gold></bold> <rainbow>Festival started!</rainbow>";

// 3. Plain format (raw text without tag parsing)
var t4 = p"Raw text without parsing & symbols";

// 4. String interpolation
var user = "Alex";
var level = 42;
player::message("Player: ${user}, Level: ${level}, Next level in: ${100 - level}");
player::message("Short interpolation: $user won!");
```

### 5.3. Dynamic Arrays (`array`)
Dynamic arrays support appending, indexing, and slicing:

```jc
var inventory = ["sword", "bow", "arrows", "potion"];

// Index access (0-based)
var first = inventory[0]; // "sword"
inventory[1] = "crossbow";

// Slice [start : end]
var combat_gear = inventory[0:2]; // ["sword", "crossbow"]

// Membership check via 'in'
if "sword" in inventory {
    player::message("Weapon equipped!");
}
```

### 5.4. Dictionaries (`map`)
Associative key-value mappings:

```jc
var prices = {
    "diamond": 100,
    "gold": 50,
    "iron": 10
};

// Access and update
var diamond_cost = prices["diamond"];
prices["emerald"] = 150;
```

### 5.5. Ranges
Numerical ranges are created using `..` (half-open) and `..=` (closed):

```jc
var r1 = 0..5;   // 0, 1, 2, 3, 4 (excludes 5)
var r2 = 0..=5;  // 0, 1, 2, 3, 4, 5 (includes 5)

// Range methods
var length = r1.len();               // 5
var has_three = r1.contains(3);      // true
var array_copy = r2.to_array();      // [0, 1, 2, 3, 4, 5]
var stepped = (0..10).step_by(2);    // steps of 2
```

### 5.6. Custom Items and NBT
Custom Minecraft item components are constructed using `m{...}`:

```jc
var super_sword = item("netherite_sword", nbt = m{
    "minecraft:unbreakable": {},
    "minecraft:custom_name": "{\"text\":\"Storm Blade\",\"color\":\"aqua\"}",
    "CustomModelData": 1001
});

player::give_items(super_sword);
```

---

## 6. Control Flow

### 6.1. Conditionals and Ternary Operators
```jc
if health <= 0 {
    player::message("You died!");
} elif health < 5 {
    player::message("Dangerously low health!");
} else {
    player::message("Condition stable");
}

// Compact ternary forms:
var status = is_alive ? "In Game" : "Spectator";
var reward = 100 if is_vip else 25;
```

### 6.2. Loops and Labels
The `for .. in` construct supports multiple iteration targets:

```jc
var items = ["apple", "bread", "steak"];

// 1. Element iteration
for food in items {
    player::message("Food: ${food}");
}

// 2. Index and value iteration
for idx, food in items {
    player::message("${idx + 1}. ${food}");
}

// 3. Range iteration
for i in 1..=5 {
    player::message("Count: ${i}");
}

// 4. Dictionary key-value iteration
var stats = {"Strength": 15, "Agility": 18};
for stat_name, stat_val in stats {
    player::message("${stat_name}: ${stat_val}");
}
```

#### Loop labels for nested loops:
```jc
'search: for x in 0..10 {
    for y in 0..10 {
        if grid[x][y] == "target" {
            player::message("Found at (${x}, ${y})!");
            break 'search; // terminates both loops
        }
    }
}
```

### 6.3. Pattern Matching: `match`
The `match` expression provides expressive pattern matching:

```jc
function handle_command(cmd: text) {
    match cmd {
        "start" | "play" => {
            player::message("Game starting!");
        }
        "help" => {
            player::message("Available commands: start, help, quit");
        }
        _ if cmd.starts_with("warp_") => {
            player::message("Teleporting to location: ${cmd}");
        }
        _ => {
            player::message("Unknown command. Type 'help'.");
        }
    }
}
```

### 6.4. Error Handling: `try` / `catch` / `throw`
```jc
try {
    if divisor == 0 {
        throw ERROR "Attempted division by zero";
    }
    var result = total / divisor;
} catch (error) {
    player::message("Calculation error: ${error}");
}
```

---

## 7. Functions, Methods, and Lambdas

### 7.1. Functions and Parameters
```jc
// Default parameters and return types
function calculate_exp(base: number, multiplier: number = 1.5) -> number {
    return base * multiplier;
}

// Pass-by-reference: mutates the caller's variable
function give_bonus(ref balance: number, bonus: number) {
    balance += bonus;
}

var my_coins = 100;
give_bonus(my_coins, 50);
// my_coins is now 150!
```

### 7.2. Inline Functions (`inline function`)
Frequent helpers can be inlined directly at call sites to avoid subroutine overhead:

```jc
inline function clamp(val: number, min: number, max: number) -> number {
    if val < min { return min; }
    if val > max { return max; }
    return val;
}
```

### 7.3. Lambdas and Functional Programming
Lambda expressions allow passing behavior as values:

```jc
var square = (x: number) => x * x;
var sum = (a: number, b: number) => a + b;

// Multi-line block lambda
var process_score = (score: number) => {
    var bonus = score > 100 ? 50 : 10;
    return score + bonus;
};

// Higher-order function
function apply_math(val: number, operation: Fn<number, number>) -> number {
    return operation(val);
}

var res = apply_math(10, (x: number) => x * 3); // 30
```

---

## 8. Object-Oriented Programming

### 8.1. Classes and Constructors
Classes bundle state and behavior:

```jc
class Vector2 {
    var x: number;
    var y: number;

    // Instance constructor
    function __init__(self: Vector2, x: number, y: number) -> Vector2 {
        self.x = x;
        self.y = y;
        return self;
    }

    // Method
    function length_sq(self: Vector2) -> number {
        return self.x * self.x + self.y * self.y;
    }
}
```

### 8.2. Interfaces (`interface`)
Interfaces define contracts for classes:

```jc
interface Damageable {
    function take_damage(self: Damageable, amount: number);
}

interface Healer {
    function heal(self: Healer, amount: number);
}

// Multiple interface inheritance
interface Combatant extends Damageable, Healer {}

class Boss implements Combatant {
    var health: number;

    function __init__(self: Boss, health: number) -> Boss {
        self.health = health;
        return self;
    }

    function take_damage(self: Boss, amount: number) {
        self.health -= amount;
        player::message("Boss took damage! Remaining: ${self.health}");
    }

    function heal(self: Boss, amount: number) {
        self.health += amount;
    }
}
```

### 8.3. Operator Overloading (Dunder Methods)
To add `+` or index access `[]` to your classes, implement the respective dunder methods:

```jc
class Point {
    var x: number;
    var y: number;

    function __init__(self: Point, x: number, y: number) -> Point {
        self.x = x;
        self.y = y;
        return self;
    }

    // Overload + (__add__)
    function __add__(self: Point, other: Point) -> Point {
        return Point(self.x + other.x, self.y + other.y);
    }

    // String formatting (__str__)
    function __str__(self: Point) -> text {
        return "Point(${self.x}, ${self.y})";
    }
}

event<player_join> {
    var p1 = Point(10, 20);
    var p2 = Point(5, 15);
    var p3 = p1 + p2; // calls p1.__add__(p2)
    player::message("Result: ${p3}"); // Point(15, 35)
}
```

---

## 9. Programming for JustMC (Minecraft)

### 9.1. Game Events
JustCode scripts execute in response to platform events:
- `event<player_join>` — Player joins the server.
- `event<player_quit>` — Player leaves the server.
- `event<player_damage_player>` — PvP damage.
- `event<player_right_click>` — Right-click action.
- `event<player_chat>` — Chat message sent.

### 9.2. Action Invocations and Selectors
Actions follow the pattern `category::action<selector>(arguments)`. Selectors define target entities:

```jc
event<player_damage_player> {
    // Send message to victim
    player::message<victim>("Damaged by: " + value::name<damager>);

    // Play sound to attacker
    player::play_sound<damager>(sound("entity.experience_orb.pickup"));

    // Global alert
    player::action_bar<all_players>("Combat in progress!");
}
```

### 9.3. Reading Game Values (`value::...`)
Dynamic game values retrieve runtime state:
```jc
var my_health = value::health<current>;
var my_coords = value::location<current>;
var online_count = value::player_count;
```

### 9.4. Parallel Processes (`process`)
Processes are background asynchronous tasks with delay support (`code::wait`):

```jc
process match_countdown(seconds: number) {
    while seconds > 0 {
        player::title<all_players>(
            m"<gold>${seconds}</gold>", 
            subtitle = "Starting soon", 
            fade_in = 0, stay = 20, fade_out = 0
        );
        code::wait(1, time_unit = "SECONDS");
        seconds -= 1;
    }

    player::title<all_players>(m"<green>START!</green>");
    game::is_match_running = true;
}

event<player_join> {
    // Spawn background process
    match_countdown(5);
}
```

---

## 10. Modules, Packages, and Testing

### 10.1. Module Organization (Edition 2026)
Organize code across multiple files:

```jc
// src/utils/math.jc
export function power_of_two(n: number) -> number {
    return 1 << n;
}

function secret_internal() {
    // unexported functions are private to this file
}
```

Import in your main file:
```jc
// src/main.jc
import "./utils/math.jc";

event<player_join> {
    var val = power_of_two(4); // 16
}
```

### 10.2. Built-in Test Runner (`jmcc test`)
Write tests directly in your codebase using the `@test` decorator:

```jc
// tests/math_test.jc
import "./src/utils/math.jc";

@test
function test_power() {
    var res = power_of_two(3);
    if res != 8 {
        throw ERROR "Invalid exponentiation result";
    }
}

@test
@should_panic(expected = "Division by zero")
function test_zero_div() {
    var bad = 10 / 0;
}
```

Run tests with a single command:
```bash
jmcc test
```
The compiler executes all tests in the `jmcmock` virtual environment and prints test results.

---

## 11. Practical Cookbook

### Recipe 1: Welcoming New Players with Persistent First-Join Tracking
```jc
event<player_join> {
    var player_name = value::name<current>;

    // Check persistent database variable
    if not s`has_joined_${player_name}` {
        s`has_joined_${player_name}` = true;

        player::message<all_players>(m"<yellow>First time joining: <bold>${player_name}</bold>!</yellow>");
        player::give_items(item("stone_sword"), item("bread", count = 16));
        player::teleport(location(0.5, 65, 0.5));
    } else {
        player::message("Welcome back, ${player_name}!");
    }
}
```

### Recipe 2: Economy Balance and Shop Purchases
```jc
function buy_item(cost: number, item_id: text) {
    var name = value::name<current>;
    var current_money = s`balance_${name}` as number;

    if current_money >= cost {
        s`balance_${name}` = current_money - cost;
        player::give_items(item(item_id));
        player::message(m"<green>Successfully purchased ${item_id} for ${cost} coins!</green>");
        player::play_sound(sound("entity.player.levelup"));
    } else {
        var needed = cost - current_money;
        player::message(m"<red>Insufficient funds! You need ${needed} more coins.</red>");
        player::play_sound(sound("entity.villager.no"));
    }
}
```

### Recipe 3: Interactive Health Regeneration Loop
```jc
process health_regen_loop() {
    while true {
        code::wait(3, time_unit = "SECONDS");

        for target in value::all_players {
            var current_hp = value::health<target>;
            var max_hp = value::max_health<target>;

            if current_hp < max_hp {
                player::set_health<target>(current_hp + 1);
                player::action_bar<target>(m"<green>+1 Health (Regeneration)</green>");
            }
        }
    }
}

event<world_start> {
    health_regen_loop();
}
```

---

## 12. Syntax Cheat Sheet

### Keywords
```jc
// Variable declarations:
var x = 10;            // line var (2026)
game var score = 0;    // session-global
save var bank = 100;   // persistent DB
inline var PI = 3.14;  // compile-time constant

// Functions & conditionals:
function calc(a: number, b: number) -> number { return a + b; }
var res = cond ? "yes" : "no";

// Loops:
for item in list { ... }
for idx, item in list { ... }
for key, val in map { ... }
for i in 0..10 { ... }
while is_active { ... }

// Pattern matching:
match value {
    1 | 2 => "few",
    _ if value > 10 => "many",
    _ => "other"
}
```

### CLI Commands
- `jmcc new <name>` — Create a new JustCode package (`--bin` or `--lib`).
- `jmcc compile` — Compile project to JSON.
- `jmcc compile --release` — Release build with optimizations.
- `jmcc compile -u` — Compile and upload module directly to Minecraft server.
- `jmcc test` — Execute test suite.
- `jmcc format .` — Format all `.jc` files in project.
- `jmcmock run out.json --event player_join` — Test compiled module in mock runtime.
