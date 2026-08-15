/// Stable catalog provider identity (e.g. `"curseforge"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProviderId(pub &'static str);

impl ProviderId {
    pub const CURSEFORGE: Self = Self("curseforge");
}
