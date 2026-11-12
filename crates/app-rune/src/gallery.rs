//! Gallery geometry and recent-speaker rotation, recovered from 93707ac.
//! CallView applies pin and spotlight priority after this stable rotation.
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const GALLERY_MAX: usize = 9;
const SWAP_AFTER_SPEAKING: Duration = Duration::from_secs(2);
const SWAP_INTERVAL: Duration = Duration::from_secs(5);
const KEEP_RECENT_ON_TOP: Duration = Duration::from_secs(20);
const TILE_ASPECT: f32 = 16.0 / 9.0;

/// The first page holds the own tiles and the speakers; later pages hold a full grid each, like Teams.
pub fn first_page_size(others: usize, own_cells: usize) -> usize {
    others.min(GALLERY_MAX.saturating_sub(own_cells).max(1))
}

pub fn gallery_pages(others: usize, own_cells: usize) -> usize {
    1 + (others - first_page_size(others, own_cells)).div_ceil(GALLERY_MAX)
}

pub fn gallery_page_range(others: usize, own_cells: usize, page: usize) -> std::ops::Range<usize> {
    let first = first_page_size(others, own_cells);
    if page == 0 {
        return 0..first;
    }
    let start = (first + (page - 1) * GALLERY_MAX).min(others);
    start..(start + GALLERY_MAX).min(others)
}

/// Columns and 16:9 tile size that make the largest tiles for `cells` on the stage.
pub fn gallery_grid(cells: usize, stage: (f32, f32), gap: f32) -> (usize, f32, f32) {
    let cells = cells.max(1);
    let mut best = (1, 0.0, 0.0);
    for columns in 1..=cells {
        let rows = cells.div_ceil(columns);
        let cell_width = (stage.0 - gap * (columns - 1) as f32) / columns as f32;
        let cell_height = (stage.1 - gap * (rows - 1) as f32) / rows as f32;
        let width = cell_width.min(cell_height * TILE_ASPECT).max(0.0);
        if width > best.1 {
            best = (columns, width, width / TILE_ASPECT);
        }
    }
    best
}

/// The gallery order of the other people. Newcomers who spoke go to the front, a steady speaker
/// moves ahead of everyone silent for 20 s, and at most one move happens per 5 s.
#[derive(Debug, Clone, Default)]
pub struct Rotation {
    order: Vec<String>,
    speaking_since: HashMap<String, Instant>,
    last_spoke: HashMap<String, Instant>,
    last_swap: Option<Instant>,
}

impl Rotation {
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// `capacity` is how many people the first page shows; returns whether the order changed.
    pub fn update(
        &mut self,
        now: Instant,
        members: &[String],
        speaking: &[String],
        capacity: usize,
    ) -> bool {
        let before = self.order.clone();
        self.speaking_since.retain(|mri, _| speaking.contains(mri));
        for mri in speaking {
            self.speaking_since.entry(mri.clone()).or_insert(now);
            self.last_spoke.insert(mri.clone(), now);
        }
        self.order.retain(|mri| members.contains(mri));
        self.last_spoke.retain(|mri, _| members.contains(mri));
        let mut arrivals: Vec<(usize, &String)> = members
            .iter()
            .enumerate()
            .filter(|(_, mri)| !self.order.contains(mri))
            .collect();
        arrivals.sort_by_key(|(index, mri)| {
            (
                std::cmp::Reverse(self.last_spoke.get(*mri).copied()),
                *index,
            )
        });
        let (spoken, silent): (Vec<String>, Vec<String>) = arrivals
            .into_iter()
            .map(|(_, mri)| mri.clone())
            .partition(|mri| self.last_spoke.contains_key(mri));
        self.order.splice(0..0, spoken);
        self.order.extend(silent);
        self.promote_speaker(now, capacity);
        self.order != before
    }

    fn promote_speaker(&mut self, now: Instant, capacity: usize) {
        if self
            .last_swap
            .is_some_and(|swapped| now.saturating_duration_since(swapped) < SWAP_INTERVAL)
        {
            return;
        }
        let mut candidates: Vec<(usize, Instant)> = self
            .order
            .iter()
            .enumerate()
            .filter_map(|(index, mri)| Some((index, *self.speaking_since.get(mri)?)))
            .filter(|(_, since)| now.saturating_duration_since(*since) >= SWAP_AFTER_SPEAKING)
            .collect();
        candidates.sort_by_key(|(index, since)| (*since, *index));
        let Some((index, target)) = candidates
            .into_iter()
            .find_map(|(index, since)| Some((index, self.promotion_target(index, since)?)))
        else {
            return;
        };
        let speaker = self.order.remove(index);
        self.order.insert(target, speaker);
        if capacity > 0 && index >= capacity {
            self.drop_least_recent(target, capacity);
        }
        self.last_swap = Some(now);
    }

    /// The first place ahead of `index` held by someone silent now and for 20 s before `since`.
    fn promotion_target(&self, index: usize, since: Instant) -> Option<usize> {
        self.order[..index].iter().position(|mri| {
            !self.speaking_since.contains_key(mri)
                && self.last_spoke.get(mri).is_none_or(|spoke| {
                    since.saturating_duration_since(*spoke) >= KEEP_RECENT_ON_TOP
                })
        })
    }

    /// A speaker from a later page pushes the least recent speaker off the first page.
    fn drop_least_recent(&mut self, newcomer: usize, capacity: usize) {
        let victim = self.order[..=capacity]
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != newcomer)
            .min_by_key(|(position, mri)| {
                (
                    self.last_spoke.get(*mri).copied(),
                    std::cmp::Reverse(*position),
                )
            })
            .map(|(position, _)| position);
        if let Some(victim) = victim {
            let dropped = self.order.remove(victim);
            self.order.insert(capacity, dropped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("p{index}")).collect()
    }

    fn speak(mris: &[&str]) -> Vec<String> {
        mris.iter().map(|mri| (*mri).to_owned()).collect()
    }

    fn visible(rotation: &Rotation, capacity: usize) -> Vec<String> {
        rotation.order()[..capacity].to_vec()
    }

    #[test]
    fn a_big_gallery_pages_through_everyone_once() {
        assert_eq!(gallery_pages(8, 1), 1);
        assert_eq!(gallery_pages(9, 1), 2);
        assert_eq!(gallery_pages(9, 0), 1);
        assert_eq!(gallery_pages(26, 1), 3);
        assert_eq!(gallery_page_range(26, 1, 0), 0..8);
        assert_eq!(gallery_page_range(26, 1, 1), 8..17);
        assert_eq!(gallery_page_range(26, 1, 2), 17..26);
        assert_eq!(gallery_page_range(0, 1, 0), 0..0);
        assert_eq!(gallery_pages(0, 1), 1);
    }

    #[test]
    fn grid_tiles_stay_sixteen_by_nine_and_fit() {
        assert_eq!(
            gallery_grid(2, (1600.0, 900.0), 20.0),
            (2, 790.0, 790.0 / TILE_ASPECT)
        );
        let (columns, width, height) = gallery_grid(9, (1600.0, 900.0), 20.0);
        assert_eq!(columns, 3);
        assert!(width * 3.0 + 40.0 <= 1600.0 && height * 3.0 + 40.0 <= 900.0);
        assert_eq!(gallery_grid(4, (500.0, 1200.0), 20.0).0, 1);
    }

    #[test]
    fn people_who_spoke_start_at_the_front_and_stay_put() {
        let mut rotation = Rotation::default();
        let now = Instant::now();
        let members = names(5);
        rotation.update(now, &members, &speak(&["p3"]), 4);
        assert_eq!(rotation.order()[0], "p3");
        let before = visible(&rotation, 4);
        rotation.update(now, &members, &speak(&["p3"]), 4);
        assert_eq!(visible(&rotation, 4), before);
    }

    #[test]
    fn an_outside_speaker_swaps_in_only_after_two_seconds_of_speech() {
        let mut rotation = Rotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], 4);
        rotation.update(start, &members, &speak(&["p5"]), 4);
        rotation.update(
            start + Duration::from_millis(1900),
            &members,
            &speak(&["p5"]),
            4,
        );
        assert!(!visible(&rotation, 4).contains(&"p5".to_owned()));
        rotation.update(start + SWAP_AFTER_SPEAKING, &members, &speak(&["p5"]), 4);
        assert!(visible(&rotation, 4).contains(&"p5".to_owned()));
    }

    #[test]
    fn a_new_speaker_moves_up_and_the_least_recent_speaker_drops_out() {
        let mut rotation = Rotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], 4);
        rotation.update(start, &members, &speak(&["p0", "p1", "p3"]), 4);
        let talking = speak(&["p5", "p0", "p1", "p3"]);
        rotation.update(start, &members, &talking, 4);
        rotation.update(start + SWAP_AFTER_SPEAKING, &members, &talking, 4);
        assert_eq!(visible(&rotation, 4), speak(&["p0", "p1", "p3", "p2"]));
        rotation.update(
            start + SWAP_AFTER_SPEAKING + SWAP_INTERVAL,
            &members,
            &talking,
            4,
        );
        assert_eq!(visible(&rotation, 4), speak(&["p0", "p1", "p3", "p5"]));
    }

    #[test]
    fn recent_speakers_stay_on_top_after_they_fall_silent() {
        let mut rotation = Rotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], 4);
        rotation.update(start, &members, &speak(&["p3"]), 4);
        let promoted = start + SWAP_AFTER_SPEAKING;
        rotation.update(promoted, &members, &speak(&["p3"]), 4);
        assert_eq!(rotation.order()[0], "p3");
        rotation.update(promoted + Duration::from_secs(1), &members, &[], 4);
        rotation.update(promoted + Duration::from_secs(31), &members, &[], 4);
        assert_eq!(rotation.order()[0], "p3");
    }

    #[test]
    fn two_people_taking_turns_keep_their_places() {
        let mut rotation = Rotation::default();
        let start = Instant::now();
        let members = names(4);
        rotation.update(start, &members, &speak(&["p2"]), 4);
        rotation.update(start + SWAP_AFTER_SPEAKING, &members, &speak(&["p2"]), 4);
        let mut now = start + SWAP_AFTER_SPEAKING;
        for _ in 0..2 {
            for speaker in ["p3", "p2"] {
                now += Duration::from_secs(1);
                rotation.update(now, &members, &speak(&[speaker]), 4);
                now += SWAP_INTERVAL;
                rotation.update(now, &members, &speak(&[speaker]), 4);
            }
        }
        assert_eq!(
            rotation.order(),
            speak(&["p2", "p3", "p0", "p1"]).as_slice()
        );
    }

    #[test]
    fn at_most_one_speaker_swaps_in_per_five_seconds() {
        let mut rotation = Rotation::default();
        let start = Instant::now();
        let members = names(7);
        rotation.update(start, &members, &[], 4);
        let talking = speak(&["p5", "p6"]);
        let count = |rotation: &Rotation| {
            visible(rotation, 4)
                .iter()
                .filter(|mri| *mri == "p5" || *mri == "p6")
                .count()
        };
        rotation.update(start, &members, &talking, 4);
        rotation.update(start + SWAP_AFTER_SPEAKING, &members, &talking, 4);
        assert_eq!(count(&rotation), 1);
        rotation.update(
            start + SWAP_AFTER_SPEAKING + Duration::from_secs(4),
            &members,
            &talking,
            4,
        );
        assert_eq!(count(&rotation), 1);
        rotation.update(
            start + SWAP_AFTER_SPEAKING + SWAP_INTERVAL,
            &members,
            &talking,
            4,
        );
        assert_eq!(count(&rotation), 2);
    }

    #[test]
    fn people_who_leave_drop_out_and_the_next_one_moves_up() {
        let mut rotation = Rotation::default();
        let now = Instant::now();
        let members = names(5);
        rotation.update(now, &members, &[], 3);
        let remaining: Vec<String> = members.into_iter().filter(|mri| mri != "p1").collect();
        rotation.update(now, &remaining, &[], 3);
        assert_eq!(visible(&rotation, 3), vec!["p0", "p2", "p3"]);
    }
}
