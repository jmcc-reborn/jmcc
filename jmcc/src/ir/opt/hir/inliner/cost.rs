pub mod inline_constants {
    pub const INSTR_COST: i32 = 5;
    pub const DEFAULT_THRESHOLD: i32 = 225;
    pub const COLD_THRESHOLD: i32 = 45;
    pub const HOT_CALLSITE_THRESHOLD: i32 = 3000;
    pub const INLINE_HINT_THRESHOLD: i32 = 325;
    pub const OPT_SIZE_THRESHOLD: i32 = 50;
    pub const OPT_MIN_SIZE_THRESHOLD: i32 = 5;
    pub const AGGRESSIVE_THRESHOLD: i32 = 250;
    pub const CALL_PENALTY: i32 = 25;
    pub const LOOP_PENALTY: i32 = 25;
    pub const SINGLE_BB_BONUS_PERCENT: i32 = 50;
    pub const LAST_CALL_TO_STATIC_BONUS: i32 = 1500;
    pub const MAX_INLINE_DEPTH: u32 = 3;
    pub const ALWAYS_INLINE_SIZE: i32 = 15;
    pub const NEVER_INLINE_SIZE: i32 = 2000;
}

#[derive(Debug, Clone)]
pub enum InlineCost {
    Always(&'static str),
    Never(&'static str),
    Cost {
        cost: i32,
        threshold: i32,
        reason: &'static str,
    },
}

impl InlineCost {
    #[inline]
    #[must_use]
    pub fn should_inline(&self) -> bool {
        let res = match self {
            InlineCost::Always(_) => true,
            InlineCost::Never(_) => false,
            InlineCost::Cost {
                cost, threshold, ..
            } => *cost < *threshold,
        };
        log::trace!("InlineCost::should_inline -> {res} ({self:?})");
        res
    }

    #[inline]
    #[must_use]
    pub const fn is_mandatory(&self) -> bool {
        matches!(self, Self::Always(_))
    }

    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Always(r) => format!("always: {r}"),
            Self::Never(r) => format!("never: {r}"),
            Self::Cost {
                cost,
                threshold,
                reason,
            } => {
                format!("cost={cost} threshold={threshold} ({reason})")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct InlineParams {
    pub default_threshold: i32,
    pub opt_size_threshold: i32,
    pub opt_min_size_threshold: i32,
    pub cold_threshold: i32,
    pub hot_callsite_threshold: i32,
    pub aggressive_threshold: i32,
}

impl Default for InlineParams {
    fn default() -> Self {
        Self {
            default_threshold: inline_constants::DEFAULT_THRESHOLD,
            opt_size_threshold: inline_constants::OPT_SIZE_THRESHOLD,
            opt_min_size_threshold: inline_constants::OPT_MIN_SIZE_THRESHOLD,
            cold_threshold: inline_constants::COLD_THRESHOLD,
            hot_callsite_threshold: inline_constants::HOT_CALLSITE_THRESHOLD,
            aggressive_threshold: inline_constants::AGGRESSIVE_THRESHOLD,
        }
    }
}

impl InlineParams {
    #[must_use]
    pub fn for_opt_level(opt_level: u8, size_opt_level: u8) -> Self {
        let default_threshold = match opt_level {
            0 => 0,
            1 => 100,
            2 => inline_constants::DEFAULT_THRESHOLD,
            _ => inline_constants::AGGRESSIVE_THRESHOLD,
        };

        let mut params = Self {
            default_threshold,
            ..Self::default()
        };

        if size_opt_level > 0 {
            params.default_threshold = if size_opt_level > 1 {
                inline_constants::OPT_MIN_SIZE_THRESHOLD
            } else {
                inline_constants::OPT_SIZE_THRESHOLD
            };
        }

        params
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Number(f64),
    Bool(bool),
    Text(String),
}
