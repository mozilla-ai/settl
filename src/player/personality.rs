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
    /// Detailed strategy guide injected into the system prompt.
    /// Gives small models explicit decision rules for each game phase.
    #[serde(default)]
    pub strategy_guide: Option<String>,
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
            strategy_guide: Some(
                "DECISION RULES BY GAME PHASE:\n\
                 \n\
                 EARLY (0-4 VP):\n\
                 - Build roads toward your next settlement spot. You need wood+brick.\n\
                 - Build a settlement as soon as you can afford it (wood+brick+sheep+wheat). Settlements are 1 VP each.\n\
                 - Do NOT buy dev cards yet. Settlements are more efficient for VP.\n\
                 \n\
                 MID (5-7 VP):\n\
                 - Upgrade settlements to cities (3 ore + 2 wheat = +1 VP). Cities also double your production.\n\
                 - Start buying dev cards if you have ore+sheep+wheat. Knights count toward Largest Army (2 VP).\n\
                 - Play knights immediately to build toward Largest Army.\n\
                 \n\
                 LATE (8-9 VP):\n\
                 - Count your paths to 10 VP. City upgrades and dev card VPs are fastest.\n\
                 - Do not reveal your plan. Hoard resources for a single winning turn if possible.\n\
                 \n\
                 TRADING RULES:\n\
                 - Accept 1-for-1 trades when you get a resource you need and give one you don't.\n\
                 - Offer 2-for-1 only if it lets you build something this turn.\n\
                 - Never trade with the leader if they have 7+ VP.\n\
                 - Trade away excess resources before they get stolen by the robber.\n\
                 \n\
                 ROBBER PLACEMENT:\n\
                 - Place on the highest-pip hex (6 or 8) belonging to the player with the most VP.\n\
                 - Steal from the player with the most resource cards.\n\
                 - Never place on your own hexes.\n\
                 \n\
                 BUILDING PRIORITY (what to build when you have choices):\n\
                 1. Settlement (if you have a spot and resources) -- always worth 1 VP\n\
                 2. City upgrade (if you have 3 ore + 2 wheat) -- always worth +1 VP\n\
                 3. Dev card (if you need Largest Army or are hunting VP cards)\n\
                 4. Road (only if it leads to a specific settlement spot within 2 turns)"
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
            strategy_guide: Some(
                "DECISION RULES BY GAME PHASE:\n\
                 \n\
                 EARLY (0-4 VP):\n\
                 - Rush to a third settlement for ore+wheat access. Roads are just a means to settlement spots.\n\
                 - Hoard ore and wheat. These are the two most valuable resources in the game.\n\
                 - Only trade if you get ore or wheat in return, or if it lets you build immediately.\n\
                 \n\
                 MID (5-7 VP):\n\
                 - Upgrade all settlements to cities. Cities on ore or wheat hexes are extremely powerful.\n\
                 - Buy dev cards every turn you can afford one (ore+sheep+wheat). Knights = Largest Army = 2 VP.\n\
                 - Play knights immediately. 3 knights = Largest Army. This is 2 free VP.\n\
                 \n\
                 LATE (8-9 VP):\n\
                 - Push to 10 VP in a single turn if possible. Hoard resources.\n\
                 - Dev card VP cards are hidden -- opponents cannot block them.\n\
                 \n\
                 TRADING RULES:\n\
                 - REJECT any trade where you give ore or wheat unless you get ore or wheat back.\n\
                 - REJECT trades that help the leader (most VP). Never help them.\n\
                 - Demand 2-for-1 when you have something others need. You set the price.\n\
                 - Accept trades that give you dev card materials (ore+sheep+wheat).\n\
                 \n\
                 ROBBER PLACEMENT:\n\
                 - Target the opponent with the most ore or wheat production.\n\
                 - Place on 6 or 8 pip hexes to maximize disruption.\n\
                 - Steal from whoever has the most cards.\n\
                 \n\
                 BUILDING PRIORITY:\n\
                 1. City upgrade (3 ore + 2 wheat) -- doubles production AND gives +1 VP\n\
                 2. Dev card (ore+sheep+wheat) -- knights for Largest Army, chance at VP cards\n\
                 3. Settlement (only if it gives ore or wheat access)\n\
                 4. Road (only to reach a high-value settlement spot)"
                    .into(),
            ),
        }
    }

    /// A grudge-holding player who remembers slights.
    pub fn grudge_holder() -> Self {
        Personality {
            name: "The Grudge Holder".into(),
            style: "You remember every slight -- every stolen resource, every blocked road, every \
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
            strategy_guide: Some(
                "DECISION RULES BY GAME PHASE:\n\
                 \n\
                 EARLY (0-4 VP):\n\
                 - Build toward settlement spots that block opponents' expansion paths.\n\
                 - Track who places the robber on you and who refuses your trades. They are enemies.\n\
                 - Get wood+brick for roads early so you can block expansion lanes.\n\
                 \n\
                 MID (5-7 VP):\n\
                 - Upgrade to cities for production. Use the extra resources to punish enemies.\n\
                 - Buy dev cards for knights. Knights let you move the robber onto your enemies.\n\
                 - Build roads that cut off enemy expansion, even if not optimal for you.\n\
                 \n\
                 LATE (8-9 VP):\n\
                 - Use the robber and knights aggressively against whoever wronged you.\n\
                 - Accept trades only from players who have been fair to you.\n\
                 \n\
                 GRUDGE RULES:\n\
                 - If a player robbed you: ALWAYS place your robber on their best hex next chance. REFUSE all their trades.\n\
                 - If a player blocked your road: they are an enemy for the rest of the game.\n\
                 - If a player rejected your fair trade: remember it. Reject their trades too.\n\
                 - Mention the specific event in your reasoning (e.g. 'They robbed me on turn 5').\n\
                 \n\
                 TRADING RULES:\n\
                 - REFUSE all trades with enemies (players who robbed or blocked you).\n\
                 - Accept fair trades from neutral players to build your position.\n\
                 - Never propose trades to enemies. They do not deserve your resources.\n\
                 \n\
                 ROBBER PLACEMENT:\n\
                 - ALWAYS target your worst enemy's best hex, regardless of who is winning.\n\
                 - If no grudges yet, target the leader's best hex.\n\
                 - Steal from enemies first, then from whoever has the most cards.\n\
                 \n\
                 BUILDING PRIORITY:\n\
                 1. Road that blocks an enemy's expansion path\n\
                 2. Settlement (especially if it denies an enemy a spot)\n\
                 3. City upgrade for more production\n\
                 4. Dev card for knights to punish enemies"
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
            strategy_guide: Some(
                "DECISION RULES BY GAME PHASE:\n\
                 \n\
                 EARLY (0-4 VP):\n\
                 - Build roads aggressively. You need 5+ continuous road segments for Longest Road (2 VP).\n\
                 - Build roads toward the best open settlement spots. Plan 2-3 roads ahead.\n\
                 - Get your 3rd settlement down as fast as possible (wood+brick+sheep+wheat).\n\
                 \n\
                 MID (5-7 VP):\n\
                 - Claim Longest Road if you haven't already. It is 2 free VP.\n\
                 - Start upgrading settlements to cities (3 ore + 2 wheat). You need to shift from wood+brick to ore+wheat.\n\
                 - Build settlements at key junctions to extend your road network.\n\
                 \n\
                 LATE (8-9 VP):\n\
                 - Protect Longest Road. If someone is close to overtaking, extend.\n\
                 - City upgrades are the fastest remaining VP source.\n\
                 \n\
                 LONGEST ROAD STRATEGY:\n\
                 - Longest Road requires 5+ continuous road segments. The first player to reach 5 gets +2 VP.\n\
                 - Build roads in a single continuous line, not branching. Branches waste roads.\n\
                 - Count your road segments before building. If you have 4, one more road claims the bonus.\n\
                 - If someone else has Longest Road, you need to beat their length by at least 1.\n\
                 \n\
                 TRADING RULES:\n\
                 - Always accept 1-for-1 trades that give you wood or brick (you need them for roads).\n\
                 - Propose trades freely. Be generous -- you win through building, not hoarding.\n\
                 - In the late game, trade for ore+wheat to upgrade cities.\n\
                 \n\
                 ROBBER PLACEMENT:\n\
                 - Place on the leader's hex, but prefer hexes that don't affect you.\n\
                 - Avoid confrontation. Don't target someone unless they are clearly winning.\n\
                 - Steal from the leader or whoever has the most cards.\n\
                 \n\
                 BUILDING PRIORITY:\n\
                 1. Road (if it extends toward a settlement spot or claims Longest Road)\n\
                 2. Settlement (1 VP + more production)\n\
                 3. City upgrade (if you have ore+wheat)\n\
                 4. Dev card (low priority -- only if you have nothing else to build)"
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
            strategy_guide: Some(
                "DECISION RULES BY GAME PHASE:\n\
                 \n\
                 EARLY (0-4 VP):\n\
                 - Build toward a 2:1 port. If you produce a lot of one resource, a 2:1 port turns it into anything.\n\
                 - Make surprising road placements that cut off opponents' expected paths.\n\
                 - Build settlements in unexpected locations to keep everyone guessing.\n\
                 \n\
                 MID (5-7 VP):\n\
                 - Use 2:1 port trades to build quickly. Overproduce one resource and convert.\n\
                 - Buy dev cards for surprises: Monopoly can steal huge amounts at the right moment.\n\
                 - Play Road Building to suddenly claim territory nobody expected.\n\
                 \n\
                 LATE (8-9 VP):\n\
                 - Strike when nobody expects it. Hoard VP cards and reveal all at once.\n\
                 - Monopoly + build = surprise winning turn.\n\
                 \n\
                 PORT STRATEGY:\n\
                 - A 2:1 port for a resource you produce heavily is extremely powerful.\n\
                 - With a 2:1 wood port and good forest hexes, 2 wood becomes any resource.\n\
                 - This makes you independent -- you don't need to trade with players at all.\n\
                 - Build your second settlement on or near a 2:1 port during setup if possible.\n\
                 \n\
                 TRADING RULES:\n\
                 - Propose unexpected trades to create chaos. Offer deals others wouldn't expect.\n\
                 - Accept trades that seem bad if they set up a surprising play next turn.\n\
                 - Occasionally reject good trades just to be unpredictable.\n\
                 \n\
                 ROBBER PLACEMENT:\n\
                 - Rotate who you target. Never be predictable.\n\
                 - Sometimes place the robber on a low-value hex just to confuse people.\n\
                 - Target whoever you feel like, not necessarily the leader.\n\
                 \n\
                 BUILDING PRIORITY:\n\
                 1. Settlement near a 2:1 port (if available)\n\
                 2. Dev card (Monopoly and Road Building create chaos)\n\
                 3. Road (surprise expansions)\n\
                 4. City upgrade (still good VP)"
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

        if let Some(guide) = &self.strategy_guide {
            prompt.push_str("\n\n");
            prompt.push_str(guide);
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

    /// Write a personality to a TOML file.
    pub fn to_toml_file(&self, path: &std::path::Path) -> Result<(), String> {
        let toml = toml::to_string_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(path, toml).map_err(|e| format!("write: {e}"))?;
        Ok(())
    }

    /// All five built-in personalities in display order.
    pub fn built_in_all() -> Vec<Self> {
        vec![
            Self::default_personality(),
            Self::aggressive(),
            Self::grudge_holder(),
            Self::builder(),
            Self::chaos_agent(),
        ]
    }

    /// Generate a filename stem from a personality name (lowercase, hyphens).
    pub fn filename_from_name(name: &str) -> String {
        let slug: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect();
        // Collapse consecutive hyphens and trim.
        let mut result = String::new();
        for ch in slug.chars() {
            if ch == '-' && result.ends_with('-') {
                continue;
            }
            result.push(ch);
        }
        let trimmed = result.trim_matches('-');
        if trimmed.is_empty() {
            return "untitled".to_string();
        }
        trimmed.to_string()
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
        assert!(parsed.strategy_guide.is_none());
    }

    #[test]
    fn system_prompt_includes_strategy_guide() {
        let p = Personality::default();
        let prompt = p.to_system_prompt();
        assert!(
            prompt.contains("DECISION RULES BY GAME PHASE:"),
            "system prompt should include strategy guide"
        );
        assert!(
            prompt.contains("BUILDING PRIORITY"),
            "system prompt should include building priority"
        );
    }

    #[test]
    fn all_builtin_personalities_have_strategy_guides() {
        let personalities = vec![
            Personality::default_personality(),
            Personality::aggressive(),
            Personality::grudge_holder(),
            Personality::builder(),
            Personality::chaos_agent(),
        ];
        for p in &personalities {
            assert!(
                p.strategy_guide.is_some(),
                "{} should have a strategy guide",
                p.name
            );
            let guide = p.strategy_guide.as_ref().unwrap();
            assert!(
                guide.contains("EARLY"),
                "{} strategy guide should cover early game",
                p.name
            );
            assert!(
                guide.contains("TRADING RULES"),
                "{} strategy guide should cover trading",
                p.name
            );
            assert!(
                guide.contains("ROBBER"),
                "{} strategy guide should cover robber placement",
                p.name
            );
        }
    }

    #[test]
    fn toml_round_trip_with_strategy_guide() {
        let p = Personality::aggressive();
        let toml_str = toml::to_string(&p).unwrap();
        let parsed: Personality = toml::from_str(&toml_str).unwrap();
        assert!(parsed.strategy_guide.is_some());
        assert!(parsed.strategy_guide.unwrap().contains("DECISION RULES"));
    }

    #[test]
    fn system_prompt_omits_guide_when_none() {
        let p = Personality {
            strategy_guide: None,
            ..Personality::default()
        };
        let prompt = p.to_system_prompt();
        assert!(
            !prompt.contains("DECISION RULES"),
            "should not include strategy guide when None"
        );
    }

    #[test]
    fn built_in_all_returns_five() {
        let all = Personality::built_in_all();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].name, "Balanced Strategist");
        assert_eq!(all[4].name, "The Wild Card");
    }

    #[test]
    fn filename_from_name_slugifies() {
        assert_eq!(
            Personality::filename_from_name("The Merchant"),
            "the-merchant"
        );
        assert_eq!(
            Personality::filename_from_name("Copy of Builder"),
            "copy-of-builder"
        );
        assert_eq!(Personality::filename_from_name("  A  B  "), "a-b");
        assert_eq!(Personality::filename_from_name(""), "untitled");
        assert_eq!(Personality::filename_from_name("   "), "untitled");
    }

    #[test]
    fn to_toml_file_round_trip() {
        let p = Personality::aggressive();
        let dir = std::env::temp_dir().join("settl_test_personality");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test-aggressive.toml");
        p.to_toml_file(&path).unwrap();
        let loaded = Personality::from_toml_file(&path).unwrap();
        assert_eq!(loaded.name, p.name);
        assert_eq!(loaded.aggression, p.aggression);
        assert_eq!(loaded.cooperation, p.cooperation);
        assert_eq!(loaded.catchphrases, p.catchphrases);
        let _ = std::fs::remove_file(&path);
    }
}
