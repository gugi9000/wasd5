use clap::Parser;
use wasd5::cards_elemental_core::{
    Card, Opponent, Side, WinCondition, bot_card_index, full_deck, next_random, refill_hands,
    shuffle, win_condition, winner_of,
};

#[derive(Debug, Parser)]
#[command(about = "Simulate Cards Elemental bots without accessing the website database")]
struct Args {
    /// Number of complete games to attempt
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..))]
    games: u32,

    /// Bot in the first seat
    #[arg(long, value_enum, default_value = "tosser")]
    player1: Opponent,

    /// Bot in the second seat
    #[arg(long, value_enum, default_value = "pro")]
    player2: Opponent,

    /// Reproducible random seed (generated and printed when omitted)
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    seed: Option<u64>,

    /// Safety limit per game; capped games are reported as unfinished, not draws
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..))]
    max_rounds: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Win(usize, WinCondition),
    Draw,
    RoundLimit,
}

struct Game {
    deck: Vec<Card>,
    discarded: Vec<Card>,
    hands: [Vec<Card>; 2],
    winnings: [Vec<Card>; 2],
    starter: usize,
}

impl Game {
    fn new(random: &mut u64) -> Self {
        let mut deck = full_deck();
        shuffle(&mut deck, random);
        let mut hands = [Vec::new(), Vec::new()];
        for _ in 0..5 {
            for hand in &mut hands {
                hand.push(deck.pop().expect("new deck has enough cards"));
            }
        }
        Self {
            deck,
            discarded: Vec::new(),
            hands,
            winnings: [Vec::new(), Vec::new()],
            starter: (next_random(random) % 2) as usize,
        }
    }

    fn play_round(
        &mut self,
        bots: [Opponent; 2],
        random: &mut u64,
    ) -> (Option<usize>, Option<Outcome>) {
        let mut played = [None, None];
        for seat in [self.starter, 1 - self.starter] {
            let index = bot_card_index(
                bots[seat],
                &self.hands[seat],
                &self.winnings[seat],
                &self.winnings[1 - seat],
                &self.discarded,
                random,
            );
            played[seat] = Some(self.hands[seat].remove(index));
        }
        let [first, second] = played.map(|card| card.expect("both players chose a card"));
        let winner = match winner_of(first, second) {
            Some(Side::You) => Some(0),
            Some(Side::Server) => Some(1),
            None => None,
        };
        if let Some(seat) = winner {
            let cards = [first, second];
            self.winnings[seat].push(cards[seat]);
            self.discarded.push(cards[1 - seat]);
            if let Some(condition) = win_condition(&self.winnings[seat]) {
                return (winner, Some(Outcome::Win(seat, condition)));
            }
            self.starter = seat;
        } else {
            self.discarded.extend([first, second]);
        }
        let [first_hand, second_hand] = &mut self.hands;
        refill_hands(
            &mut self.deck,
            &mut self.discarded,
            first_hand,
            second_hand,
            random,
        );
        let outcome = if self.hands.iter().any(Vec::is_empty) {
            Some(Outcome::Draw)
        } else {
            None
        };
        (winner, outcome)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Stats {
    games: u64,
    wins: [u64; 2],
    draws: u64,
    unfinished: u64,
    rounds: u64,
    min_rounds: Option<u32>,
    max_rounds: u32,
    round_wins: [u64; 2],
    round_draws: u64,
    all_elements: [u64; 2],
    matching_cards: [u64; 2],
    both: [u64; 2],
}

impl Stats {
    fn record_game(&mut self, rounds: u32, outcome: Outcome) {
        self.games += 1;
        self.rounds += u64::from(rounds);
        self.min_rounds = Some(self.min_rounds.map_or(rounds, |min| min.min(rounds)));
        self.max_rounds = self.max_rounds.max(rounds);
        match outcome {
            Outcome::Draw => self.draws += 1,
            Outcome::RoundLimit => self.unfinished += 1,
            Outcome::Win(seat, condition) => {
                self.wins[seat] += 1;
                match condition {
                    WinCondition::AllElements => self.all_elements[seat] += 1,
                    WinCondition::MatchingCards => self.matching_cards[seat] += 1,
                    WinCondition::Both => self.both[seat] += 1,
                }
            }
        }
    }
}

fn simulate(args: &Args, seed: u64) -> Stats {
    let mut random = seed;
    let mut stats = Stats::default();
    for _ in 0..args.games {
        let mut game = Game::new(&mut random);
        for round in 1..=args.max_rounds {
            let (winner, outcome) = game.play_round([args.player1, args.player2], &mut random);
            if let Some(seat) = winner {
                stats.round_wins[seat] += 1;
            } else {
                stats.round_draws += 1;
            }
            if let Some(outcome) = outcome {
                stats.record_game(round, outcome);
                break;
            }
            if round == args.max_rounds {
                stats.record_game(round, Outcome::RoundLimit);
            }
        }
    }
    stats
}

fn report(args: &Args, seed: u64, stats: &Stats) -> String {
    let mut text = format!(
        "Cards Elemental: P1 {} vs P2 {}\n\
         Games: {} | Seed: {} | Round limit/game: {}\n\
         Rounds spent: {} | Average/game: {:.2} | Shortest: {} | Longest: {}\n\
         Drawn games: {} | Unfinished (round limit): {}\n\
         Drawn rounds: {}\n\n\
         {:<30} {:>12} {:>12}\n",
        args.player1,
        args.player2,
        stats.games,
        seed,
        args.max_rounds,
        stats.rounds,
        stats.rounds as f64 / stats.games as f64,
        stats.min_rounds.unwrap_or(0),
        stats.max_rounds,
        stats.draws,
        stats.unfinished,
        stats.round_draws,
        "Per player",
        format!("P1 {}", args.player1),
        format!("P2 {}", args.player2),
    );
    for (label, values) in [
        ("Game wins", stats.wins),
        ("Game losses", [stats.wins[1], stats.wins[0]]),
        ("Game draws", [stats.draws; 2]),
        ("Round wins", stats.round_wins),
        ("Round losses", [stats.round_wins[1], stats.round_wins[0]]),
        ("Round draws", [stats.round_draws; 2]),
        ("Wins: all three elements", stats.all_elements),
        ("Wins: same element + colour", stats.matching_cards),
        ("Wins: both conditions", stats.both),
    ] {
        text.push_str(&format!(
            "{label:<30} {:>12} {:>12}\n",
            values[0], values[1]
        ));
    }
    text.push_str(&format!(
        "{:<30} {:>11.2}% {:>11.2}%\n",
        "Game win rate (all attempts)",
        stats.wins[0] as f64 * 100.0 / stats.games as f64,
        stats.wins[1] as f64 * 100.0 / stats.games as f64,
    ));
    text.push_str("\nWinning-condition counts are exclusive; 'both' is counted separately.\n");
    text
}

fn main() {
    let args = Args::parse();
    let seed = args.seed.unwrap_or_else(|| {
        let bytes = *uuid::Uuid::new_v4().as_bytes();
        u64::from_le_bytes(bytes[..8].try_into().expect("eight seed bytes")).max(1)
    });
    let stats = simulate(&args, seed);
    print!("{}", report(&args, seed, &stats));
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasd5::cards_elemental_core::{Colour, Element};

    #[test]
    fn defaults_options_and_invalid_counts() {
        let args = Args::try_parse_from(["sim"]).unwrap();
        assert_eq!(args.games, 10_000);
        assert_eq!(args.player1, Opponent::Tosser);
        assert_eq!(args.player2, Opponent::Pro);
        for args in [
            vec!["sim", "--games", "0"],
            vec!["sim", "--games", "-1"],
            vec!["sim", "--max-rounds", "0"],
            vec!["sim", "--seed", "0"],
            vec!["sim", "--player1", "unknown"],
        ] {
            assert!(Args::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn repeatable_matchups_and_consistent_totals() {
        for first in [Opponent::Tosser, Opponent::Pro, Opponent::Pro2] {
            for second in [Opponent::Tosser, Opponent::Pro, Opponent::Pro2] {
                let mut args = Args::try_parse_from(["sim", "--games", "50"]).unwrap();
                args.player1 = first;
                args.player2 = second;
                let stats = simulate(&args, 42);
                assert_eq!(stats, simulate(&args, 42));
                assert_eq!(stats.games, 50);
                assert_eq!(
                    stats.wins.iter().sum::<u64>() + stats.draws + stats.unfinished,
                    50
                );
                assert_eq!(
                    stats.round_wins.iter().sum::<u64>() + stats.round_draws,
                    stats.rounds
                );
                for seat in 0..2 {
                    assert_eq!(
                        stats.all_elements[seat] + stats.matching_cards[seat] + stats.both[seat],
                        stats.wins[seat]
                    );
                }
                assert!(report(&args, 42, &stats).contains("Wins: same element + colour"));
            }
        }
    }

    #[test]
    fn capped_games_are_not_reported_as_draws() {
        let args = Args::try_parse_from(["sim", "--games", "10", "--max-rounds", "1"]).unwrap();
        let stats = simulate(&args, 42);
        assert_eq!(stats.unfinished, 10);
        assert_eq!(stats.draws, 0);
        assert_eq!(stats.rounds, 10);
        assert_eq!(stats.min_rounds, Some(1));
        assert_eq!(stats.max_rounds, 1);
    }

    #[test]
    fn drawn_round_recycles_cards_and_keeps_starter() {
        let card = Card {
            element: Element::Fire,
            colour: Colour::Red,
            value: 1,
        };
        let mut game = Game {
            deck: Vec::new(),
            discarded: Vec::new(),
            hands: [
                vec![card],
                vec![Card {
                    colour: Colour::Blue,
                    ..card
                }],
            ],
            winnings: [Vec::new(), Vec::new()],
            starter: 1,
        };
        let (winner, outcome) = game.play_round([Opponent::Tosser; 2], &mut 42);
        assert_eq!(winner, None);
        assert_eq!(outcome, None);
        assert_eq!(game.starter, 1);
        assert_eq!(game.hands[0].len(), 1);
        assert_eq!(game.hands[1].len(), 1);
        assert!(game.discarded.is_empty());
    }

    #[test]
    fn win_conditions_and_draws_are_counted_separately() {
        let mut stats = Stats::default();
        for condition in [
            WinCondition::AllElements,
            WinCondition::MatchingCards,
            WinCondition::Both,
        ] {
            stats.record_game(5, Outcome::Win(0, condition));
        }
        stats.record_game(6, Outcome::Draw);
        stats.record_game(7, Outcome::RoundLimit);
        assert_eq!(stats.wins, [3, 0]);
        assert_eq!(stats.all_elements, [1, 0]);
        assert_eq!(stats.matching_cards, [1, 0]);
        assert_eq!(stats.both, [1, 0]);
        assert_eq!(stats.draws, 1);
        assert_eq!(stats.unfinished, 1);
    }
}
