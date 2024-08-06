use std::collections::{HashMap, VecDeque, HashSet};

use libp2p::PeerId;

#[derive(Debug)]
pub struct RequestsInProgress {
    map: HashMap<PeerId, usize>,
    queues_vector: Vec<VecDeque<PeerId>>,
}

impl RequestsInProgress {
    pub fn new() -> Self {
        let queues_vector = vec![VecDeque::new(), VecDeque::new()];
        Self {
            map: HashMap::new(),
            queues_vector,
        }
    }

    pub fn push_req(&mut self, item: &PeerId) -> bool {
        // Check if peer exists in hashmap
        if let Some(&pos) = self.map.get(item) {
            // If exists delete from actual position in vector of queues
            self.queues_vector[pos].retain(|x| x != item);
            let new_pos = pos + 1;
            // Check if exists queue in new position
            if new_pos >= self.queues_vector.len() {
                // If if does not exists, create new queue with it
                let mut new_queue: VecDeque<PeerId> = VecDeque::new();
                new_queue.push_back(item.clone());
                self.queues_vector.push(new_queue);
            }
            else {
                // If it exists, push item to the queue of that position
                self.queues_vector[new_pos].push_back(item.clone());
            }
            self.map.insert(*item, new_pos);
            println!("Pushed: {:?} from pos: {:?} to pos: {:?}", item, pos, new_pos);
            println!("Actual state");
            println!("{:?}", self);
            true
        } else {
            // If peer does not exists in hashmap, push it to the first queue
            self.queues_vector[1].push_back(item.clone());
            self.map.insert(item.clone(), 1);
            println!("Pushed new peer: {:?} to pos: {:?}", item, 1);
            println!("Actual state");
            println!("{:?}", self);
            true
        }

    }

    pub fn pop_req(&mut self, item: &PeerId) -> bool {
        // Check if peer exists in hashmap
        if let Some(&pos) = self.map.get(item) {
            // If exists delete from actual position in vector of queues
            self.queues_vector[pos].retain(|x| x != item);            

            let new_pos = if pos == 0 {
                pos
            } else {
                pos - 1
            };

            // Check if new position is different from actual position (when actual is 0)
            if new_pos != pos {
                self.queues_vector[new_pos].push_back(item.clone());
            }
            self.map.insert(item.clone(), new_pos);
            println!("Popped: {:?} from pos: {:?} to pos: {:?}", item, pos, new_pos);
            println!("Actual state");
            println!("{:?}", self);
            true
        } else {
            false
        }
    }

    pub fn get_peer(& self, providers: &HashSet<PeerId>) -> Option<PeerId> {
        for queue in &self.queues_vector {
            if let Some(item) = queue.front() {
                if providers.contains(item) {
                    println!("Requested peer...");
                    println!("Actual state");
                    println!("{:?}", self);
                    println!("First peer from providers found: {:?}", item);
                    return Some(item.clone());
                }
            }
        }
        None
    }
}
