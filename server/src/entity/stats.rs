/// Stats for an entity
///
/// Stats modify some aspects of an entity or
/// track something about the entity (e.g.
/// happiness).
pub struct Stats {}

/// Marks the variant of stats used by this entity
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct StatVariant {
    /// The unique id for this variant
    pub id: u8,
    _priv: (),
}

/// Marks the variant of stats used by this entity
#[derive(PartialEq, Eq, Clone, Copy)]
pub struct Stat {
    /// The variant this stat belongs too
    pub variant: StatVariant,
    /// The unqiue index for this stat
    pub index: usize,
    _priv: (),
}

macro_rules! consume_ident {
    ($thing:ident) => {
        0
    };
}

macro_rules! count_idents {
    (
        $($name:ident),*
    ) => (
        0 $(+ consume_ident!($name) + 1isize)*
    );
}

macro_rules! const_max {
    (
        $($val:expr),*
    ) => (
        const_max!(@max $($val),*);
    );
    (
        @max $cur:expr
    ) => (
        $cur
    );
    (
        @max $cur:expr, $($val:expr),*
    ) => (
        const_max!(@do ($cur), const_max!(@max $($val),*))
    );
    (
        @do $a:expr, $b:expr
    ) => (
        (($a)-((($a)-($b))&((($a)-($b))>>31)))
    );
}

macro_rules! create_variants {
    (prev($prev:ident), $name:ident) => (
        #[allow(non_upper_case_globals)]
        /// A stat variant
        pub const $name: StatVariant = StatVariant{id: Self::$prev.id + 1, _priv: ()};
    );
    (prev($prev:ident), $name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        /// A stat variant
        pub const $name: StatVariant = StatVariant{id: Self::$prev.id + 1, _priv: ()};
        create_variants!(prev($name), $($n),+);
    );
    ($name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        /// A stat variant
        pub const $name: StatVariant = StatVariant{id: 0, _priv: ()};
        create_variants!(prev($name), $($n),+);
    );
    ($name:ident) => (
        #[allow(non_upper_case_globals)]
        /// A stat variant
        pub const $name: StatVariant = StatVariant{id: 0, _priv: ()};
    );
}

macro_rules! create_stats {
    (group($group:expr) prev($prev:ident), $name:ident) => (
        #[allow(non_upper_case_globals)]
        /// A stat key
        pub const $name: Stat = Stat {
            variant: $group,
            index: Self::$prev.index + 1,
            _priv: (),
        };
    );
    (group($group:expr) prev($prev:ident), $name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        /// A stat key
        pub const $name: Stat = Stat {
            variant: $group,
            index: Self::$prev.index + 1,
            _priv: (),
        };
        create_stats!(group($group) prev($name), $($n),+);
    );
    (group($group:expr) $name:ident, $($n:ident),+) => (
        #[allow(non_upper_case_globals)]
        /// A stat key
        pub const $name: Stat = Stat {
            variant: $group,
            index: 0,
            _priv: (),
        };
        create_stats!(group($group) prev($name), $($n),+);
    );
    (group($group:expr) $name:ident) => (
        #[allow(non_upper_case_globals)]
        /// A stat key
        pub const $name: Stat = Stat {
            variant: $group,
            index: 0,
            _priv: (),
        };
    );
}

macro_rules! def_stats {
    ($(
$group:ident = $gstr:expr => {
    $(
        $sname:ident = $sstr:expr, $sdesc:expr, $def:expr,
    )*
}
    )*) => {
impl Stats {
    /// The max number of possible stats for a single entity
    #[allow(clippy::double_parens)]
    pub const MAX: usize = const_max!($(
       consume_ident!($group) + count_idents!( $($sname),* )
    ),*) as usize;

    create_variants!($($group),*);

    $(
        create_stats!(group(Self::$group) $($sname),*);
    )*
}

impl StatVariant {
    /// Converts a raw id for a variant into a variant
    pub fn from_id(id: u8) -> Option<StatVariant> {
        let id = StatVariant{id, _priv: ()};
        match id {
            $(
            Stats::$group => Some(Stats::$group),
            )*
            _ => None,
        }
    }
    /// Returns the matching stat variant
    pub fn from_str(val: &str) -> Option<StatVariant> {
        match val {
        $(
            $gstr => Some(Stats::$group),
        )*
            _ => None,
        }
    }

    /// Returns the string form of this stat variant
    pub fn as_string(&self) -> &'static str {
        match *self {
            $(
                Stats::$group => $gstr,
            )*
            _ => unreachable!(),
        }
    }

    /// Returns a list of stats that this variant has
    pub fn stats(&self) -> &'static [Stat] {
        match *self {
            $(
                Stats::$group => &[$(
                    Stats::$sname
                ),*],
            )*
            _ => unreachable!(),
        }
    }
}

impl Stat {
    /// Converts a raw id for a variant into a variant
    pub fn from_index(variant: StatVariant, index: usize) -> Option<Stat> {
        let stat = Stat{variant, index, _priv: ()};
        match stat {
        $(
            $(
                Stats::$sname => Some(Stats::$sname),
            )*
        )*
            _ => None,
        }
    }

    /// Returns the matching stat for the passed variant and string
    /// if any.
    pub fn from_str(variant: StatVariant, val: &str) -> Option<Stat> {
        match (variant, val) {
        $(
            $(
                (Stats::$group, $sstr) => Some(Stats::$sname),
            )*
        )*
            _ => None,
        }
    }

    /// Returns the string form of this stat
    pub fn as_string(&self) -> &'static str {
        match *self {
            $(
                $(
                    Stats::$sname => $sstr,
                )*
            )*
            _ => unreachable!(),
        }
    }

    /// Returns a string suitable for putting in a tooltip
    pub fn tooltip_string(&self) -> &'static str {
        match *self {
            $(
                $(
                    Stats::$sname => $sdesc,
                )*
            )*
            _ => unreachable!(),
        }
    }

    /// Returns a suitable default value for a stat
    pub fn default_value(&self) -> f32 {
        match *self {
            $(
                $(
                    Stats::$sname => $def,
                )*
            )*
            _ => -99.0,
        }
    }
}
    };
}

def_stats! {
    STUDENT = "student" => {
        STUDENT_HAPPINESS = "happiness", "How happy the *student* is about their time at the #UniverCity#", 1.0,
        STUDENT_HUNGER = "hunger", "How hungry the *student* is. Hunger will negatively affect happiness if low", 1.0,
        STUDENT_SKILL = "skill", "How skilled the *student* is in general", 0.5,
    }
    PROFESSOR = "professor" => {
        PROFESSOR_HAPPINESS = "happiness", "How happy the *professor* about their time teaching", 1.0,
        PROFESSOR_FATIGUE = "fatigue", "How tired the *professor*. Can be recovered by resting", 1.0,
        PROFESSOR_CONTROL = "control", "How well the *professor* can control a class of students. The greater the control the more students they can handle", 0.6,
        PROFESSOR_SKILL = "skill", "How skilled the *professor* is at teaching", 0.6,
        // Hidden stat - How happy the entity is with doing their job.
        //               When low they'll ask for a raise/bonus.
        //               When zero they'll quit.
        PROFESSOR_JOB_SATISFACTION = "job_satisfaction", "*", 1.0,
    }
    JANITOR = "janitor" => {
        JANITOR_HAPPINESS = "happiness", "How happy the *janitor* is about their job", 1.0,
    }
    OFFICE_WORKER = "office_worker" => {
        OFFICE_WORKER_HAPPINESS = "happiness", "How happy the *worker* is about their job", 1.0,
    }
}
