use std::collections::HashSet;

use crate::core::BindingId;

#[derive(Default)]
pub struct Refusals {
    refused: HashSet<BindingId>,
    followings: Vec<Following>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Following {
    leader: BindingId,
    follower: BindingId,
}

impl Refusals {
    pub fn refuse(&mut self, binding: BindingId) {
        self.refused.insert(binding);
    }

    pub fn follow(&mut self, follower: BindingId, leader: BindingId) {
        self.followings.push(Following { leader, follower });
    }

    pub fn spread(mut self) -> HashSet<BindingId> {
        self.followings.sort_unstable();
        let mut pending: Vec<BindingId> = self.refused.iter().copied().collect();
        while let Some(leader) = pending.pop() {
            for follower in Self::followers(&self.followings, leader) {
                if self.refused.insert(follower) {
                    pending.push(follower);
                }
            }
        }
        self.refused
    }

    fn followers(sorted: &[Following], leader: BindingId) -> impl Iterator<Item = BindingId> + '_ {
        let first = sorted.partition_point(|following| following.leader < leader);
        sorted[first..]
            .iter()
            .take_while(move |following| following.leader == leader)
            .map(|following| following.follower)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Refusals;
    use crate::core::{BindingArena, BindingId, BindingInfo, BindingKind};

    struct RefusalFixture {
        bindings: BindingArena,
        refusals: Refusals,
    }

    impl RefusalFixture {
        fn new() -> Self {
            Self {
                bindings: BindingArena::default(),
                refusals: Refusals::default(),
            }
        }

        fn parameter(&mut self, name: &str) -> BindingId {
            self.bindings
                .add(BindingInfo::new(name.into(), BindingKind::Local, None))
        }

        fn spread(self) -> HashSet<BindingId> {
            self.refusals.spread()
        }
    }

    #[test]
    fn a_refusal_reaches_every_follower_down_a_chain() {
        let mut fixture = RefusalFixture::new();
        let inner = fixture.parameter("inner");
        let middle = fixture.parameter("middle");
        let outer = fixture.parameter("outer");
        fixture.refusals.follow(outer, middle);
        fixture.refusals.follow(middle, inner);
        fixture.refusals.refuse(inner);
        assert_eq!(fixture.spread(), HashSet::from([inner, middle, outer]));
    }

    #[test]
    fn a_cycle_that_nothing_refuses_keeps_every_member() {
        let mut fixture = RefusalFixture::new();
        let even = fixture.parameter("even");
        let odd = fixture.parameter("odd");
        fixture.refusals.follow(even, odd);
        fixture.refusals.follow(odd, even);
        assert_eq!(fixture.spread(), HashSet::new());
    }

    #[test]
    fn a_refusal_that_enters_a_cycle_refuses_every_member() {
        let mut fixture = RefusalFixture::new();
        let pad = fixture.parameter("pad");
        let fill = fixture.parameter("fill");
        let push = fixture.parameter("push");
        fixture.refusals.follow(pad, fill);
        fixture.refusals.follow(fill, pad);
        fixture.refusals.follow(pad, push);
        fixture.refusals.refuse(push);
        assert_eq!(fixture.spread(), HashSet::from([pad, fill, push]));
    }

    #[test]
    fn a_leader_that_is_not_refused_keeps_its_follower() {
        let mut fixture = RefusalFixture::new();
        let leader = fixture.parameter("leader");
        let follower = fixture.parameter("follower");
        let refused = fixture.parameter("refused");
        fixture.refusals.follow(follower, leader);
        fixture.refusals.refuse(refused);
        assert_eq!(fixture.spread(), HashSet::from([refused]));
    }
}
