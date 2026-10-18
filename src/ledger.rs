//! Remembers which bot reply answered which message, so replies can be
//! dismissed by their asker and cleaned up when the question is deleted.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplyRecord {
    pub reply_id: u64,
    /// The message that triggered the reply. `None` for slash commands.
    pub trigger_id: Option<u64>,
    pub owner_id: u64,
}

/// A bounded, oldest-first record of recent replies.
#[derive(Debug)]
pub struct ReplyLedger {
    capacity: usize,
    records: VecDeque<ReplyRecord>,
}

impl ReplyLedger {
    /// A capacity of zero is treated as one.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            records: VecDeque::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, record: ReplyRecord) {
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn owner_of(&self, reply_id: u64) -> Option<u64> {
        self.records
            .iter()
            .find(|record| record.reply_id == reply_id)
            .map(|record| record.owner_id)
    }

    pub fn remove_reply(&mut self, reply_id: u64) -> Option<ReplyRecord> {
        let index = self.records.iter().position(|record| record.reply_id == reply_id)?;
        self.records.remove(index)
    }

    pub fn take_by_trigger(&mut self, trigger_id: u64) -> Option<ReplyRecord> {
        let index = self
            .records
            .iter()
            .position(|record| record.trigger_id == Some(trigger_id))?;
        self.records.remove(index)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(reply_id: u64, trigger_id: Option<u64>, owner_id: u64) -> ReplyRecord {
        ReplyRecord {
            reply_id,
            trigger_id,
            owner_id,
        }
    }

    #[test]
    fn finds_owner_by_reply() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, Some(1), 100));
        assert_eq!(ledger.owner_of(10), Some(100));
        assert_eq!(ledger.owner_of(11), None);
    }

    #[test]
    fn evicts_the_oldest_when_full() {
        let mut ledger = ReplyLedger::new(2);
        ledger.insert(record(10, Some(1), 100));
        ledger.insert(record(11, Some(2), 101));
        ledger.insert(record(12, Some(3), 102));
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger.owner_of(10), None);
        assert_eq!(ledger.owner_of(12), Some(102));
    }

    #[test]
    fn zero_capacity_still_keeps_the_latest() {
        let mut ledger = ReplyLedger::new(0);
        ledger.insert(record(10, None, 100));
        ledger.insert(record(11, None, 101));
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.owner_of(11), Some(101));
    }

    #[test]
    fn removes_by_reply_and_by_trigger() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, Some(1), 100));
        ledger.insert(record(11, None, 101));

        assert_eq!(ledger.take_by_trigger(1), Some(record(10, Some(1), 100)));
        assert_eq!(ledger.take_by_trigger(1), None);
        assert_eq!(ledger.remove_reply(11), Some(record(11, None, 101)));
        assert_eq!(ledger.remove_reply(11), None);
        assert!(ledger.is_empty());
    }

    #[test]
    fn slash_replies_never_match_a_trigger() {
        let mut ledger = ReplyLedger::new(4);
        ledger.insert(record(10, None, 100));
        assert_eq!(ledger.take_by_trigger(10), None);
        assert_eq!(ledger.len(), 1);
    }
}
