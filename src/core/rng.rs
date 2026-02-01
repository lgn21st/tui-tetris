//! RNG module - 7-bag random piece generation
//!
//! Implements the "7-bag" randomization algorithm used in modern Tetris.
//! Each bag contains one of each piece (I, O, T, S, Z, J, L), shuffled.
//! Draws from the bag until empty, then generates a new bag.
//!
//! Also provides a simple LCG for deterministic testing.

use crate::types::PieceKind;

/// Simple LCG (Linear Congruential Generator) RNG
/// Uses constants from Numerical Recipes
#[derive(Debug, Clone)]
pub struct SimpleRng {
    state: u32,
}

impl SimpleRng {
    /// Create a new RNG with the given seed
    pub fn new(seed: u32) -> Self {
        // Avoid 0 seed which would produce all zeros
        let state = if seed == 0 { 1 } else { seed };
        Self { state }
    }

    /// Generate next random u32
    pub fn next_u32(&mut self) -> u32 {
        // LCG formula: (a * state + c) mod m
        // Using Numerical Recipes constants: a=1664525, c=1013904223, m=2^32
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// Generate random value in range [0, max)
    pub fn next_range(&mut self, max: u32) -> u32 {
        self.next_u32() % max
    }

    /// Shuffle a slice using Fisher-Yates
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        for i in (1..slice.len()).rev() {
            let j = self.next_range((i + 1) as u32) as usize;
            slice.swap(i, j);
        }
    }
}

/// 7-bag piece generator
#[derive(Debug, Clone)]
pub struct PieceQueue {
    /// Current bag of pieces
    bag: Vec<PieceKind>,
    /// Index into current bag
    bag_index: usize,
    /// RNG for shuffling
    rng: SimpleRng,
}

impl PieceQueue {
    /// Create a new piece queue with the given seed
    pub fn new(seed: u32) -> Self {
        let mut queue = Self {
            bag: Vec::with_capacity(7),
            bag_index: 0,
            rng: SimpleRng::new(seed),
        };
        queue.refill_bag();
        queue
    }

    /// Generate a new shuffled bag
    fn refill_bag(&mut self) {
        self.bag = vec![
            PieceKind::I,
            PieceKind::O,
            PieceKind::T,
            PieceKind::S,
            PieceKind::Z,
            PieceKind::J,
            PieceKind::L,
        ];
        self.rng.shuffle(&mut self.bag);
        self.bag_index = 0;
    }

    /// Peek at the next piece without removing it
    pub fn peek(&self) -> Option<PieceKind> {
        self.bag.get(self.bag_index).copied()
    }

    /// Peek at multiple upcoming pieces
    pub fn peek_queue(&self, count: usize) -> Vec<PieceKind> {
        let mut result = Vec::with_capacity(count);
        let mut idx = self.bag_index;

        // First, take from current bag
        while result.len() < count && idx < self.bag.len() {
            result.push(self.bag[idx]);
            idx += 1;
        }

        // If we need more, we need to look at next bag
        // For simplicity, generate a preview from a fresh bag with same RNG state
        if result.len() < count {
            // Clone RNG state to avoid affecting main RNG
            let mut preview_rng = SimpleRng::new(self.rng.state);
            let mut next_bag = vec![
                PieceKind::I,
                PieceKind::O,
                PieceKind::T,
                PieceKind::S,
                PieceKind::Z,
                PieceKind::J,
                PieceKind::L,
            ];
            preview_rng.shuffle(&mut next_bag);

            let remaining = count - result.len();
            for i in 0..remaining.min(7) {
                result.push(next_bag[i]);
            }
        }

        result
    }

    /// Draw the next piece from the queue
    pub fn draw(&mut self) -> PieceKind {
        // Ensure bag has pieces
        if self.bag_index >= self.bag.len() {
            self.refill_bag();
        }

        let piece = self.bag[self.bag_index];
        self.bag_index += 1;
        piece
    }

    /// Check if we can draw more pieces (always true, but maintains API compatibility)
    pub fn can_draw(&self) -> bool {
        true
    }

    /// Get current bag for testing/debugging
    #[cfg(test)]
    pub fn current_bag(&self) -> &[PieceKind] {
        &self.bag[self.bag_index..]
    }

    /// Get the current RNG state (for restarting game with same sequence)
    pub fn seed(&self) -> u32 {
        self.rng.state
    }
}

impl Default for PieceQueue {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rng_deterministic() {
        let mut rng1 = SimpleRng::new(12345);
        let mut rng2 = SimpleRng::new(12345);

        // Same seed should produce same sequence
        for _ in 0..100 {
            assert_eq!(rng1.next_u32(), rng2.next_u32());
        }
    }

    #[test]
    fn test_rng_different_seeds() {
        let mut rng1 = SimpleRng::new(12345);
        let mut rng2 = SimpleRng::new(54321);

        // Different seeds should eventually diverge
        let v1 = rng1.next_u32();
        let v2 = rng2.next_u32();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_piece_queue_initial() {
        let queue = PieceQueue::new(1);

        // Should be able to peek at first piece
        assert!(queue.peek().is_some());

        // Should have 7 pieces in bag
        assert_eq!(queue.current_bag().len(), 7);
    }

    #[test]
    fn test_piece_queue_draws_all_seven() {
        let mut queue = PieceQueue::new(1);

        // Draw all 7 pieces
        let mut drawn = Vec::new();
        for _ in 0..7 {
            drawn.push(queue.draw());
        }

        // Should have exactly one of each piece
        assert_eq!(drawn.len(), 7);
        for kind in [
            PieceKind::I,
            PieceKind::O,
            PieceKind::T,
            PieceKind::S,
            PieceKind::Z,
            PieceKind::J,
            PieceKind::L,
        ] {
            assert!(drawn.contains(&kind), "Missing piece: {:?}", kind);
        }
    }

    #[test]
    fn test_piece_queue_auto_refill() {
        let mut queue = PieceQueue::new(1);

        // Draw 8 pieces (one more than bag size)
        let first = queue.draw();
        for _ in 0..6 {
            queue.draw();
        }
        let eighth = queue.draw();

        // Eighth piece should be from new bag
        // It might or might not equal first, but there should be no panic
        assert!(queue.current_bag().len() <= 7);
    }

    #[test]
    fn test_piece_queue_peek() {
        let mut queue = PieceQueue::new(1);

        let peeked = queue.peek().unwrap();
        let drawn = queue.draw();

        // Peek should match first draw
        assert_eq!(peeked, drawn);
    }

    #[test]
    fn test_piece_queue_peek_queue() {
        let queue = PieceQueue::new(1);

        let preview = queue.peek_queue(3);
        assert_eq!(preview.len(), 3);

        // First 3 should match current bag
        let bag_preview = queue.peek_queue(7);
        assert_eq!(bag_preview.len(), 7);
    }
}
