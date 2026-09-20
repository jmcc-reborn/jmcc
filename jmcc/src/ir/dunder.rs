//! Magic dunder (`__...__`) method normalization and equivalence mapping across English and Russian.

/// A group of equivalent dunder method names (canonical English name + synonyms/aliases).
#[derive(Debug, Clone, Copy)]
pub struct DunderGroup {
    /// Canonical name (typically the standard English dunder name, e.g. `__init__`).
    pub canonical: &'static str,
    /// Aliases in English and Russian.
    pub aliases: &'static [&'static str],
}

impl DunderGroup {
    /// Returns an iterator over all names in this group (canonical name followed by aliases).
    pub fn all_names(&self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.canonical).chain(self.aliases.iter().copied())
    }

    /// Checks if a given name belongs to this dunder group.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        self.canonical == name || self.aliases.contains(&name)
    }
}

/// Catalog of all supported dunder method equivalence groups.
pub const DUNDER_GROUPS: &[DunderGroup] = &[
    // Constructor / Initialization
    DunderGroup {
        canonical: "__init__",
        aliases: &[
            "__конструктор__",
            "__иниц__",
            "__инициализатор__",
            "__создать__",
        ],
    },
    // Addition: +
    DunderGroup {
        canonical: "__add__",
        aliases: &["__сложить__", "__плюс__", "__прибавить__"],
    },
    // In-place addition: +=
    DunderGroup {
        canonical: "__iadd__",
        aliases: &[
            "__прибавить_присвоить__",
            "__плюс_равно__",
            "__присвоить_сложить__",
            "__сложить_присвоить__",
        ],
    },
    // Subtraction: -
    DunderGroup {
        canonical: "__subtract__",
        aliases: &["__вычесть__", "__минус__", "__отнять__", "__sub__"],
    },
    // In-place subtraction: -=
    DunderGroup {
        canonical: "__isubtract__",
        aliases: &[
            "__вычесть_присвоить__",
            "__минус_равно__",
            "__присвоить_вычесть__",
            "__isub__",
        ],
    },
    // Multiplication: *
    DunderGroup {
        canonical: "__multiply__",
        aliases: &["__умножить__", "__умножение__", "__mul__"],
    },
    // In-place multiplication: *=
    DunderGroup {
        canonical: "__imultiply__",
        aliases: &[
            "__умножить_присвоить__",
            "__умножить_равно__",
            "__присвоить_умножить__",
            "__imul__",
        ],
    },
    // Division: /
    DunderGroup {
        canonical: "__divide__",
        aliases: &["__разделить__", "__поделить__", "__деление__", "__div__"],
    },
    // In-place division: /=
    DunderGroup {
        canonical: "__idivide__",
        aliases: &[
            "__разделить_присвоить__",
            "__разделить_равно__",
            "__присвоить_разделить__",
            "__idiv__",
        ],
    },
    // Remainder: %
    DunderGroup {
        canonical: "__remainder__",
        aliases: &[
            "__остаток__",
            "__остаток_от_деления__",
            "__модуль__",
            "__mod__",
        ],
    },
    // In-place remainder: %=
    DunderGroup {
        canonical: "__iremainder__",
        aliases: &["__остаток_присвоить__", "__присвоить_остаток__", "__imod__"],
    },
    // Power: ^ or **
    DunderGroup {
        canonical: "__pow__",
        aliases: &["__степень__", "__возвести_в_степень__"],
    },
    // In-place power: ^=
    DunderGroup {
        canonical: "__ipow__",
        aliases: &["__степень_присвоить__", "__присвоить_степень__"],
    },
    // Equality: ==
    DunderGroup {
        canonical: "__equals__",
        aliases: &["__равно__", "__равенство__", "__eq__"],
    },
    // Inequality: !=
    DunderGroup {
        canonical: "__not_equals__",
        aliases: &["__не_равно__", "__неравно__", "__неравенство__", "__ne__"],
    },
    // Greater: >
    DunderGroup {
        canonical: "__greater__",
        aliases: &["__больше__", "__gt__"],
    },
    // Less: <
    DunderGroup {
        canonical: "__less__",
        aliases: &["__меньше__", "__lt__"],
    },
    // Greater or equal: >=
    DunderGroup {
        canonical: "__greater_or_equals__",
        aliases: &["__больше_или_равно__", "__больше_равно__", "__ge__"],
    },
    // Less or equal: <=
    DunderGroup {
        canonical: "__less_or_equals__",
        aliases: &["__меньше_или_равно__", "__меньше_равно__", "__le__"],
    },
    // Membership: in
    DunderGroup {
        canonical: "__contains__",
        aliases: &["__содержит__", "__содержится__", "__в__"],
    },
    // Indexing: []
    DunderGroup {
        canonical: "__subscript__",
        aliases: &[
            "__индекс__",
            "__элемент__",
            "__взять__",
            "__getitem__",
            "__setitem__",
        ],
    },
    // Slicing: [..]
    DunderGroup {
        canonical: "__slice__",
        aliases: &["__срез__"],
    },
    // Bitwise AND: &
    DunderGroup {
        canonical: "__bitand__",
        aliases: &["__bit_and__", "__бит_и__", "__побитовое_и__"],
    },
    // Bitwise OR: |
    DunderGroup {
        canonical: "__bitor__",
        aliases: &["__bit_or__", "__бит_или__", "__побитовое_или__"],
    },
    // Bitwise XOR: ^
    DunderGroup {
        canonical: "__bitxor__",
        aliases: &[
            "__bit_xor__",
            "__бит_искл_или__",
            "__искл_или__",
            "__побитовое_искл_или__",
        ],
    },
    // Shift left: <<
    DunderGroup {
        canonical: "__lshift__",
        aliases: &["__shl__", "__сдвиг_влево__"],
    },
    // Shift right: >>
    DunderGroup {
        canonical: "__rshift__",
        aliases: &["__shr__", "__сдвиг_вправо__"],
    },
    // Logical AND
    DunderGroup {
        canonical: "__and__",
        aliases: &["__и__"],
    },
    // Logical OR
    DunderGroup {
        canonical: "__or__",
        aliases: &["__или__"],
    },
    // Logical NOT
    DunderGroup {
        canonical: "__not__",
        aliases: &["__не__"],
    },
    // Unary minus / Negation
    DunderGroup {
        canonical: "__neg__",
        aliases: &["__отрицание__", "__минус_унарный__"],
    },
    // Set attribute
    DunderGroup {
        canonical: "__set_attribute__",
        aliases: &["__установить_атрибут__", "__задать_атрибут__"],
    },
    // Get attribute
    DunderGroup {
        canonical: "__get_attribute__",
        aliases: &["__получить_атрибут__", "__взять_атрибут__"],
    },
    // String representation
    DunderGroup {
        canonical: "__str__",
        aliases: &["__строка__", "__текст__", "__text__"],
    },
    // Length
    DunderGroup {
        canonical: "__len__",
        aliases: &["__длина__", "__размер__"],
    },
    // Iterator
    DunderGroup {
        canonical: "__iter__",
        aliases: &["__итератор__", "__итер__"],
    },
    // Next item
    DunderGroup {
        canonical: "__next__",
        aliases: &["__следующий__", "__след__"],
    },
];

/// Finds the dunder group that matches the given name.
#[must_use]
pub fn find_dunder_group(name: &str) -> Option<&'static DunderGroup> {
    DUNDER_GROUPS.iter().find(|group| group.matches(name))
}

/// Returns the canonical English dunder name for the given identifier, if it is a known dunder.
#[must_use]
pub fn canonical_dunder_name(name: &str) -> Option<&'static str> {
    find_dunder_group(name).map(|g| g.canonical)
}

/// Returns `true` if `name` represents a class constructor / initializer (`__init__`, `__конструктор__`, `__иниц__`, etc.).
#[must_use]
pub fn is_init_dunder(name: &str) -> bool {
    canonical_dunder_name(name) == Some("__init__")
}

/// Returns an iterator over all equivalent names in the same dunder group as `name`.
pub fn dunder_equivalents(name: &str) -> impl Iterator<Item = &'static str> {
    find_dunder_group(name)
        .into_iter()
        .flat_map(DunderGroup::all_names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_dunder_equivalents() {
        assert!(is_init_dunder("__init__"));
        assert!(is_init_dunder("__конструктор__"));
        assert!(is_init_dunder("__иниц__"));
        assert!(is_init_dunder("__инициализатор__"));
        assert!(is_init_dunder("__создать__"));
        assert_eq!(canonical_dunder_name("__конструктор__"), Some("__init__"));
        assert_eq!(canonical_dunder_name("__иниц__"), Some("__init__"));
    }

    #[test]
    fn test_operator_dunder_equivalents() {
        assert_eq!(canonical_dunder_name("__сложить__"), Some("__add__"));
        assert_eq!(canonical_dunder_name("__плюс__"), Some("__add__"));
        assert_eq!(canonical_dunder_name("__вычесть__"), Some("__subtract__"));
        assert_eq!(canonical_dunder_name("__минус__"), Some("__subtract__"));
        assert_eq!(canonical_dunder_name("__индекс__"), Some("__subscript__"));
        assert_eq!(canonical_dunder_name("__срез__"), Some("__slice__"));
    }
}
