//! Utilities for timestamps and durations.

use crate::message::Time;
use chrono::{naive::NaiveDateTime, Utc};
use std::fmt::{Debug, Formatter};

/// A clock knows what time it currently is.
pub trait Clock: 'static + Clone + Copy + Debug + Sync + Send {
    /// Get the current time.
    fn now(&self) -> Time;
}

/// A real clock returns the current time relative to the Unix epoch.
#[derive(Clone, Copy, Default)]
pub struct RealClock {}

impl Clock for RealClock {
    fn now(&self) -> Time {
        Time::from_naive_date_time(NaiveDateTime::from_timestamp(Utc::now().timestamp(), 0))
    }
}

impl Debug for RealClock {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.now())
    }
}

#[doc(hidden)]
pub mod test_util {
    use super::Clock;
    use crate::message::Time;
    use chrono::{NaiveDate, NaiveDateTime};

    /// A mock clock for use in testing.
    #[derive(Clone, Copy, Debug)]
    pub struct MockClock {
        /// The time that this clock will always return from [`Self::now`]
        pub(crate) current_time: NaiveDateTime,
    }

    impl Clock for MockClock {
        fn now(&self) -> Time {
            Time::from_naive_date_time(self.current_time)
        }
    }

    impl Default for MockClock {
        fn default() -> Self {
            Self {
                current_time: NaiveDate::from_ymd(2001, 9, 9).and_hms(1, 46, 40),
            }
        }
    }
}
