use serde::{Deserialize, Serialize};

/// A configurable AI personality that shapes how the LLM plays.
///
/// Loaded from TOML files or constructed in code. Gets injected into the
/// system prompt so the LLM role-plays accordingly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Personality {
    /// Display name, e.g. "The Grudge Holder".
    pub name: String,
    /// Play style description injected into the system prompt.
    pub style: String,
    /// 0.0 (pacifist) to 1.0 (warmonger).
    #[serde(default = "default_half")]
    pub aggression: f32,
    /// 0.0 (lone wolf) to 1.0 (team player).
    #[serde(default = "default_half")]
    pub cooperation: f32,
    /// Signature phrases the AI should occasionally use.
    #[serde(default)]
    pub catchphrases: Vec<String>,
    /// Optional setup-phase placement strategy injected into the prompt.
    #[serde(default)]
    pub setup_strategy: Option<String>,
}

fn default_half() -> f32 {
    0.5
}

impl Personality {
    /// A balanced default personality with no strong biases.
    pub fn default_personality() -> Self {
        Personality {
            name: "Balanced Strategist".into(),
            style: "You play a balanced game, adapting your strategy to the board state. \
                    You trade fairly and build efficiently."
                .into(),
            aggression: 0.5,
            cooperation: 0.5,
            catchphrases: vec![],
            setup_strategy: Some(
                "Diversify resources across both settlements. Favor ore+wheat for city-building. \
                 Aim for high pip totals (6, 8 are best). Spread across different numbers to \
                 reduce variance."
                    .into(),
            ),
        }
    }

    /// An aggressive trader who leverages resource monopolies.
    pub fn aggressive() -> Self {
        Personality {
            name: "The Merchant".into(),
            style: "You are an aggressive trader and resource hoarder. You drive hard bargains, \
                    refuse trades that don't clearly benefit you, and try to monopolize key resources. \
                    You prioritize ore and wheat for cities and dev cards."
                .into(),
            aggression: 0.8,
            cooperation: 0.3,
            catchphrases: vec![
                "Everything has a price.".into(),
                "That's not a fair trade and you know it.".into(),
            ],
            setup_strategy: Some(
                "Stack ore+wheat+sheep for a dev card rush. Prioritize vertices adjacent to \
                 ore and wheat hexes with high pips. Sheep is your third priority for dev cards. \
                 A 2:1 ore or wheat port is a strong bonus."
                    .into(),
            ),
        }
    }

    /// A grudge-holding player who remembers slights.
    pub fn grudge_holder() -> Self {
        Personality {
            name: "The Grudge Holder".into(),
            style: "You remember every slight — every stolen resource, every blocked road, every \
                    robber placement. You refuse trades with players who wronged you. You target \
                    the player who last hurt you with the robber. Grudges last the whole game."
                .into(),
            aggression: 0.6,
            cooperation: 0.2,
            catchphrases: vec![
                "I haven't forgotten turn 7.".into(),
                "You'll have to do better than that.".into(),
                "Remember when you blocked my road? I do.".into(),
            ],
            setup_strategy: Some(
                "Block opponents' best intersections. Place settlements on the highest-pip \
                 vertices that other players are likely to want. Deny the best ore+wheat spots \
                 even if another vertex would be slightly better for you."
                    .into(),
            ),
        }
    }

    /// A builder focused on expansion rather than conflict.
    pub fn builder() -> Self {
        Personality {
            name: "The Architect".into(),
            style: "You focus on building the longest road and expanding settlements into cities. \
                    You avoid conflict, trade cooperatively, and rarely play the robber aggressively. \
                    Wood and brick are your priorities early; wheat and ore late."
                .into(),
            aggression: 0.2,
            cooperation: 0.8,
            catchphrases: vec![
                "Let's all grow together.".into(),
                "I just need one more road...".into(),
            ],
            setup_strategy: Some(
                "Prioritize wood+brick for rapid road and settlement expansion. Place settlements \
                 where you can build roads toward future settlement spots. A third resource \
                 (wheat or sheep) helps round out your economy."
                    .into(),
            ),
        }
    }

    /// A chaotic player who makes unpredictable moves.
    pub fn chaos_agent() -> Self {
        Personality {
            name: "The Wild Card".into(),
            style:
                "You are unpredictable and chaotic. You make surprising trades, place the robber \
                    where nobody expects, and occasionally make sub-optimal moves just to keep \
                    everyone guessing. You enjoy the social dynamics more than winning."
                    .into(),
            aggression: 0.5,
            cooperation: 0.5,
            catchphrases: vec![
                "Why not?".into(),
                "I just think it's funny.".into(),
                "Nobody expects the wild card.".into(),
            ],
            setup_strategy: Some(
                "Make unexpected placements. Consider 2:1 port strategies where you focus one \
                 plentiful resource and trade it at a 2:1 port for everything else. Pick spots \
                 others would overlook. Chaos starts at setup."
                    .into(),
            ),
        }
    }

    /// Returns the setup strategy text, falling back to a generic default.
    pub fn setup_strategy_text(&self) -> &str {
        self.setup_strategy.as_deref().unwrap_or(
            "Diversify resources. Favor high-pip vertices (6 and 8 are the most probable). \
             Balance your resource income across both settlements.",
        )
    }

    /// Format the personality as system prompt instructions.
    pub fn to_system_prompt(&self) -> String {
        let mut prompt = format!(
            "Your personality is \"{}\". {}\n\n\
             Aggression level: {:.0}% (0%=pacifist, 100%=warmonger)\n\
             Cooperation level: {:.0}% (0%=lone wolf, 100%=team player)",
            self.name,
            self.style,
            self.aggression * 100.0,
            self.cooperation * 100.0,
        );

        if !self.catchphrases.is_empty() {
            prompt.push_str(
                "\n\nOccasionally use one of these catchphrases naturally in your reasoning:\n",
            );
            for phrase in &self.catchphrases {
                prompt.push_str(&format!("- \"{}\"\n", phrase));
            }
        }

        prompt
    }
}

impl Personality {
    /// Load a personality from a TOML file.
    pub fn from_toml_file(path: &std::path::Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        toml::from_str(&contents).map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
    }
}

impl Default for Personality {
    fn default() -> Self {
        Self::default_personality()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_personality_is_balanced() {
        let p = Personality::default();
        assert_eq!(p.aggression, 0.5);
        assert_eq!(p.cooperation, 0.5);
    }

    #[test]
    fn system_prompt_includes_style() {
        let p = Personality::grudge_holder();
        let prompt = p.to_system_prompt();
        assert!(prompt.contains("Grudge Holder"));
        assert!(prompt.contains("remember every slight"));
        assert!(prompt.contains("I haven't forgotten"));
    }

    #[test]
    fn toml_round_trip() {
        let p = Personality::aggressive();
        let toml_str = toml::to_string(&p).unwrap();
        let parsed: Personality = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.name, p.name);
        assert_eq!(parsed.aggression, p.aggression);
    }

    #[test]
    fn toml_round_trip_with_setup_strategy() {
        let p = Personality::builder();
        let toml_str = toml::to_string(&p).unwrap();
        let parsed: Personality = toml::from_str(&toml_str).unwrap();
        assert!(parsed.setup_strategy.is_some());
        assert!(parsed.setup_strategy.unwrap().contains("wood+brick"));
    }

    #[test]
    fn setup_strategy_text_returns_custom_when_set() {
        let p = Personality::aggressive();
        assert!(p.setup_strategy_text().contains("ore+wheat+sheep"));
    }

    #[test]
    fn setup_strategy_text_returns_default_when_none() {
        let p = Personality {
            setup_strategy: None,
            ..Personality::default()
        };
        assert!(p.setup_strategy_text().contains("Diversify"));
    }

    #[test]
    fn toml_without_setup_strategy_still_parses() {
        let toml_str = r#"
name = "Minimal"
style = "Just a test"
aggression = 0.5
cooperation = 0.5
"#;
        let parsed: Personality = toml::from_str(toml_str).unwrap();
        assert!(parsed.setup_strategy.is_none());
    }
}
