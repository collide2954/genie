use crate::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stain {
    Gneg,
    Gpos(u8),
    Acen,
    Gvar,
    Stalk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Band {
    pub span: Region,
    pub name: String,
    pub stain: Stain,
}

impl Stain {
    pub fn parse(text: &str) -> Self {
        match text {
            "acen" => Self::Acen,
            "gvar" => Self::Gvar,
            "stalk" => Self::Stalk,
            _ => match text.strip_prefix("gpos") {
                Some(depth) => Self::Gpos(depth.parse().map_or(100, |depth: u8| depth.min(100))),
                None => Self::Gneg,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Stain;

    #[test]
    fn stains_read_from_giemsa_names() {
        for (text, stain) in [
            ("gneg", Stain::Gneg),
            ("gpos25", Stain::Gpos(25)),
            ("gpos33", Stain::Gpos(33)),
            ("gpos100", Stain::Gpos(100)),
            ("gpos", Stain::Gpos(100)),
            ("acen", Stain::Acen),
            ("gvar", Stain::Gvar),
            ("stalk", Stain::Stalk),
            ("", Stain::Gneg),
            ("other", Stain::Gneg),
            ("gpos250", Stain::Gpos(100)),
        ] {
            assert_eq!(Stain::parse(text), stain, "{text}");
        }
    }
}
