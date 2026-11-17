//! The device table's order: which column it is sorted by, and how a header click changes that.

use std::cmp::Ordering;

use crate::link::Device;

/// A column of the device table, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Name,
    Rssi,
    Address,
}

impl Column {
    pub const ALL: [Column; 3] = [Column::Name, Column::Rssi, Column::Address];

    pub fn title(self) -> &'static str {
        match self {
            Column::Name => "Name",
            Column::Rssi => "RSSI",
            Column::Address => "Address",
        }
    }
}

/// How the table is sorted. The default is strongest signal first: during a scan the device in your hand is usually
/// the closest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub column: Column,
    pub ascending: bool,
}

impl Default for Sort {
    fn default() -> Sort {
        Sort { column: Column::Rssi, ascending: false }
    }
}

impl Sort {
    /// After a click on `clicked`: the active column flips; another starts in the order useful for it (strongest
    /// signal first, names and addresses A to Z).
    pub fn clicked(self, clicked: Column) -> Sort {
        if self.column == clicked {
            Sort { ascending: !self.ascending, ..self }
        } else {
            Sort { column: clicked, ascending: clicked != Column::Rssi }
        }
    }

    /// A column's header, marked with the arrow when it is the active one.
    pub fn header(self, column: Column) -> String {
        match (self.column == column, self.ascending) {
            (false, _) => column.title().to_string(),
            (true, true) => format!("{}  ▲", column.title()),
            (true, false) => format!("{}  ▼", column.title()),
        }
    }

    /// Two devices in this order. Two rules ignore the direction: unnamed devices sort last on a name sort (a field scan
    /// turns up dozens, and they would bury the ones being sought), and ties break on the address, ascending (the list
    /// repaints twice a second and RSSI ties constantly; a tiebreak that flipped would reorder equal rows on each click).
    pub fn compare(self, a: &Device, b: &Device) -> Ordering {
        if self.column == Column::Name {
            match (a.name.is_empty(), b.name.is_empty()) {
                (true, false) => return Ordering::Greater,
                (false, true) => return Ordering::Less,
                _ => {}
            }
        }
        let by = match self.column {
            Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Column::Rssi => a.rssi.cmp(&b.rssi),
            Column::Address => a.addr.cmp(&b.addr),
        };
        let by = if self.ascending { by } else { by.reverse() };
        by.then_with(|| a.addr.cmp(&b.addr))
    }

    pub fn apply(self, devices: &mut [Device]) {
        devices.sort_by(|a, b| self.compare(a, b));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn d(addr: &str, name: &str, rssi: i16) -> Device {
        Device { addr: addr.into(), name: name.into(), rssi, seen: Instant::now() }
    }

    fn addrs(v: &[Device]) -> Vec<&str> {
        v.iter().map(|d| d.addr.as_str()).collect()
    }

    #[test]
    fn strongest_signal_first_by_default() {
        let mut v = vec![d("a", "x", -80), d("b", "y", -40), d("c", "z", -60)];
        Sort::default().apply(&mut v);
        assert_eq!(addrs(&v), ["b", "c", "a"]);
    }

    #[test]
    fn unnamed_devices_stay_last_either_way() {
        let mut v = vec![d("a", "", -1), d("b", "Beta", -2), d("c", "alpha", -3)];
        for ascending in [true, false] {
            Sort { column: Column::Name, ascending }.apply(&mut v);
            assert_eq!(v[2].addr, "a");
        }
        Sort { column: Column::Name, ascending: true }.apply(&mut v);
        assert_eq!(addrs(&v), ["c", "b", "a"]);
    }

    #[test]
    fn ties_break_on_the_address_ascending_in_both_directions() {
        let mut v = vec![d("c", "", -50), d("a", "", -50), d("b", "", -50)];
        for ascending in [true, false] {
            Sort { column: Column::Rssi, ascending }.apply(&mut v);
            assert_eq!(addrs(&v), ["a", "b", "c"]);
        }
    }

    #[test]
    fn a_click_flips_or_switches() {
        let s = Sort::default();
        assert_eq!(s.clicked(Column::Rssi), Sort { column: Column::Rssi, ascending: true });
        assert_eq!(s.clicked(Column::Name), Sort { column: Column::Name, ascending: true });
        assert_eq!(s.clicked(Column::Name).clicked(Column::Rssi), Sort { column: Column::Rssi, ascending: false });
    }

    #[test]
    fn the_active_header_carries_the_arrow() {
        let s = Sort::default();
        assert_eq!(s.header(Column::Rssi), "RSSI  ▼");
        assert_eq!(s.header(Column::Name), "Name");
        assert_eq!(s.clicked(Column::Address).header(Column::Address), "Address  ▲");
    }
}
