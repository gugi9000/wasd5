use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Element {
    Fire,
    Ice,
    Water,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    Red,
    Green,
    Blue,
    Yellow,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Card {
    pub element: Element,
    pub colour: Colour,
    pub value: u8,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    You,
    Server,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Opponent {
    #[default]
    Tosser,
    Pro,
    Pro2,
}

impl std::fmt::Display for Opponent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Tosser => "Tosser",
            Self::Pro => "Pro",
            Self::Pro2 => "Pro2",
        })
    }
}

pub fn next_random(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545f4914f6cdd1d)
}

pub fn shuffle(deck: &mut [Card], state: &mut u64) {
    for index in (1..deck.len()).rev() {
        let swap = (next_random(state) as usize) % (index + 1);
        deck.swap(index, swap);
    }
}

pub fn full_deck() -> Vec<Card> {
    let colours = [Colour::Blue, Colour::Red, Colour::Green, Colour::Yellow];
    let elements = [Element::Fire, Element::Ice, Element::Water];
    let mut deck = Vec::with_capacity(120);
    for value in 1..=10 {
        for element in elements {
            for colour in colours {
                deck.push(Card {
                    element,
                    colour,
                    value,
                });
            }
        }
    }
    deck
}

pub fn bot_card_index(
    opponent: Opponent,
    hand: &[Card],
    winnings: &[Card],
    opponent_winnings: &[Card],
    discarded: &[Card],
    random_state: &mut u64,
) -> usize {
    assert!(!hand.is_empty(), "a bot must have a card to play");
    match opponent {
        Opponent::Tosser => (next_random(random_state) as usize) % hand.len(),
        Opponent::Pro => pro_card_index(hand, winnings, opponent_winnings, discarded),
        Opponent::Pro2 => strategic_card_index(hand, winnings, opponent_winnings, discarded, true),
    }
}

fn collection_value(winnings: &[Card]) -> f64 {
    if has_won(winnings) {
        return 1000.0;
    }
    let elements: std::collections::HashSet<_> = winnings.iter().map(|card| card.element).collect();
    let matching = winnings
        .iter()
        .map(|card| {
            winnings
                .iter()
                .filter(|other| other.element == card.element && other.colour == card.colour)
                .count()
        })
        .max()
        .unwrap_or(0);
    // Both routes to victory matter; two matching cards are a one-card threat.
    let element_progress = [0.0, 8.0, 28.0][elements.len()];
    let matching_progress = [0.0, 4.0, 24.0][matching];
    element_progress + matching_progress
}

pub fn pro_card_index(
    hand: &[Card],
    winnings: &[Card],
    opponent_winnings: &[Card],
    discarded: &[Card],
) -> usize {
    strategic_card_index(hand, winnings, opponent_winnings, discarded, false)
}

fn has_matching_pair(winnings: &[Card]) -> bool {
    winnings.iter().enumerate().any(|(index, card)| {
        winnings[..index]
            .iter()
            .any(|other| other.element == card.element && other.colour == card.colour)
    })
}

fn pro2_collection_value(winnings: &[Card]) -> f64 {
    if has_won(winnings) {
        return 1000.0;
    }
    let elements: std::collections::HashSet<_> = winnings.iter().map(|card| card.element).collect();
    let mut value = [0.0, 8.0, 28.0][elements.len()];
    for (index, card) in winnings.iter().enumerate() {
        if winnings[..index]
            .iter()
            .any(|other| other.element == card.element && other.colour == card.colour)
        {
            continue;
        }
        let count = winnings
            .iter()
            .filter(|other| other.element == card.element && other.colour == card.colour)
            .count();
        // Only use this stronger colour valuation after an actual pair is won.
        value += [0.0, 6.0, 48.0][count];
    }
    value
}

fn strategic_card_index(
    hand: &[Card],
    winnings: &[Card],
    opponent_winnings: &[Card],
    discarded: &[Card],
    defensive: bool,
) -> usize {
    let gain_for = |pile: &[Card], card: Card| {
        let mut next = pile.to_vec();
        next.push(card);
        // Gate on the existing pile, not a hypothetical pair from this capture.
        if defensive && has_matching_pair(pile) {
            pro2_collection_value(&next) - pro2_collection_value(pile)
        } else {
            collection_value(&next) - collection_value(pile)
        }
    };
    // Do not inspect the opponent's hand, their pending card, or the real deck.
    let unseen: Vec<_> = full_deck()
        .into_iter()
        .filter(|card| {
            !hand.contains(card)
                && !winnings.contains(card)
                && !opponent_winnings.contains(card)
                && !discarded.contains(card)
        })
        .map(|card| {
            let threat = gain_for(opponent_winnings, card);
            let mut next = opponent_winnings.to_vec();
            next.push(card);
            let terminal = defensive && has_won(&next);
            (card, threat, terminal)
        })
        .collect();
    let mut best_index = 0;
    let mut best_score = f64::NEG_INFINITY;
    for (index, candidate) in hand.iter().enumerate() {
        let gain = gain_for(winnings, *candidate);
        let mut score = 0.0;
        let mut total_weight = 0.0;
        for (opponent_card, threat, terminal) in &unseen {
            // Expect opponents to favour cards that advance their own collection.
            let weight = 1.0 + threat.min(40.0) / 8.0;
            let utility = if beats(*candidate, *opponent_card) {
                gain
            } else if beats(*opponent_card, *candidate) {
                // Losing the game matters more than speculative collection progress.
                if *terminal { -10_000.0 } else { -*threat }
            } else {
                0.0
            };
            score += weight * utility;
            total_weight += weight;
        }
        if total_weight > 0.0 {
            score /= total_weight;
        }
        if score > best_score {
            best_score = score;
            best_index = index;
        }
    }
    best_index
}

pub fn beats(first: Card, second: Card) -> bool {
    if first.element == second.element {
        return first.value > second.value;
    }
    matches!(
        (first.element, second.element),
        (Element::Fire, Element::Ice)
            | (Element::Ice, Element::Water)
            | (Element::Water, Element::Fire)
    )
}

pub fn winner_of(first: Card, second: Card) -> Option<Side> {
    if beats(first, second) {
        Some(Side::You)
    } else if beats(second, first) {
        Some(Side::Server)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinCondition {
    AllElements,
    MatchingCards,
    Both,
}

pub fn win_condition(winnings: &[Card]) -> Option<WinCondition> {
    let elements: std::collections::HashSet<_> = winnings.iter().map(|card| card.element).collect();
    let mut counts = [[0u8; 4]; 3];
    let mut matching = false;
    for card in winnings {
        let element = match card.element {
            Element::Fire => 0,
            Element::Ice => 1,
            Element::Water => 2,
        };
        let colour = match card.colour {
            Colour::Red => 0,
            Colour::Green => 1,
            Colour::Blue => 2,
            Colour::Yellow => 3,
        };
        counts[element][colour] += 1;
        matching |= counts[element][colour] >= 3;
    }
    match (elements.len() == 3, matching) {
        (true, true) => Some(WinCondition::Both),
        (true, false) => Some(WinCondition::AllElements),
        (false, true) => Some(WinCondition::MatchingCards),
        (false, false) => None,
    }
}

pub fn has_won(winnings: &[Card]) -> bool {
    win_condition(winnings).is_some()
}

pub fn refill_hands(
    deck: &mut Vec<Card>,
    discarded: &mut Vec<Card>,
    first_hand: &mut Vec<Card>,
    second_hand: &mut Vec<Card>,
    random_state: &mut u64,
) {
    if deck.len() < 2 {
        deck.append(discarded);
        shuffle(deck, random_state);
    }
    if deck.len() >= 2 {
        first_hand.push(deck.pop().expect("deck has a card"));
        second_hand.push(deck.pop().expect("deck has two cards"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(element: Element, colour: Colour, value: u8) -> Card {
        Card {
            element,
            colour,
            value,
        }
    }

    fn pro2(hand: &[Card], winnings: &[Card], opponent: &[Card]) -> usize {
        bot_card_index(Opponent::Pro2, hand, winnings, opponent, &[], &mut 42)
    }

    #[test]
    fn pro2_waits_for_a_won_pair_and_completes_both_winning_sets() {
        let fire = card(Element::Fire, Colour::Red, 10);
        let ice = card(Element::Ice, Colour::Blue, 10);
        let water = card(Element::Water, Colour::Green, 10);
        let hand = [fire, ice, water];
        let singleton = [card(Element::Fire, Colour::Red, 1)];
        assert_ne!(pro2(&hand, &singleton, &[]), 0);
        let pair = [singleton[0], card(Element::Fire, Colour::Red, 2)];
        assert_eq!(pro2(&hand, &pair, &[]), 0);
        let elements = [singleton[0], card(Element::Ice, Colour::Green, 1)];
        assert_eq!(pro2(&hand, &elements, &[]), 2);
    }

    #[test]
    fn pro2_colour_bonus_requires_an_existing_matching_pair() {
        let fire = card(Element::Fire, Colour::Red, 1);
        let singleton = [fire];
        let pair = [fire, card(Element::Fire, Colour::Red, 2)];
        let mixed = [fire, card(Element::Fire, Colour::Blue, 2)];
        assert!(!has_matching_pair(&[]));
        assert!(!has_matching_pair(&singleton));
        assert!(!has_matching_pair(&mixed));
        assert!(has_matching_pair(&pair));
        assert!(pro2_collection_value(&pair) > collection_value(&pair));
    }

    #[test]
    fn pro2_blocks_either_imminent_win_instead_of_racing_with_a_weak_card() {
        let elements = [
            card(Element::Fire, Colour::Red, 1),
            card(Element::Ice, Colour::Green, 1),
        ];
        let hand = [
            card(Element::Water, Colour::Blue, 1),
            card(Element::Ice, Colour::Blue, 10),
        ];
        assert_eq!(pro2(&hand, &elements, &elements), 1);
        let pair = [
            card(Element::Fire, Colour::Red, 1),
            card(Element::Fire, Colour::Red, 2),
        ];
        let own = [
            card(Element::Ice, Colour::Blue, 1),
            card(Element::Water, Colour::Green, 1),
        ];
        let hand = [
            card(Element::Fire, Colour::Yellow, 1),
            card(Element::Water, Colour::Blue, 10),
        ];
        assert_eq!(pro2(&hand, &own, &pair), 1);
    }

    #[test]
    fn winning_conditions_distinguish_both_rules() {
        let fire = Card {
            element: Element::Fire,
            colour: Colour::Red,
            value: 1,
        };
        let ice = Card {
            element: Element::Ice,
            ..fire
        };
        let water = Card {
            element: Element::Water,
            ..fire
        };
        assert_eq!(win_condition(&[fire, ice]), None);
        assert_eq!(
            win_condition(&[fire, ice, water]),
            Some(WinCondition::AllElements)
        );
        let pair = [fire, Card { value: 2, ..fire }];
        assert_eq!(win_condition(&pair), None);
        let matching = [pair[0], pair[1], Card { value: 3, ..fire }];
        assert_eq!(win_condition(&matching), Some(WinCondition::MatchingCards));
        let both = [matching[0], matching[1], matching[2], ice, water];
        assert_eq!(win_condition(&both), Some(WinCondition::Both));
    }
}
