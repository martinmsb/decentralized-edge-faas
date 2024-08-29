use std::collections::{HashMap, HashSet};

use libp2p::PeerId;

#[derive(Debug)]
struct PeerData {
    vector_position: usize,
    manycall_in_progress: u64,
}
#[derive(Debug)]
pub struct RequestsInProgress {
    map: HashMap<PeerId, PeerData>,
    queues_vector: Vec<Vec<PeerId>>,
}

impl RequestsInProgress {
    pub fn new() -> Self {
        let queues_vector = vec![Vec::new(), Vec::new()];
        Self {
            map: HashMap::new(),
            queues_vector,
        }
    }

    pub fn push_req(&mut self, item: &PeerId, manycall_first_use: bool) -> bool {
        // Check if peer exists in hashmap
        if let Some(data) = self.map.get(item) {
            // If exists delete from actual position in vector of queues
            let pos = data.vector_position;
            let mp = data.manycall_in_progress;

            self.queues_vector[pos].retain(|x| x != item);
            let new_pos = pos + 1;
            let new_mp = if manycall_first_use { mp + 1 } else { mp };
            // Check if exists queue in new position
            if new_pos >= self.queues_vector.len() {
                // If if does not exists, create new queue with it
                let mut new_queue: Vec<PeerId> = Vec::new();
                new_queue.push(item.clone());
                self.queues_vector.push(new_queue);
            }
            else {
                // If it exists, push item to the queue of that position
                self.queues_vector[new_pos].push(item.clone());
            }
            self.map.insert(item.clone(), PeerData{vector_position: new_pos, manycall_in_progress: new_mp});
            println!("Pushed: {:?} from pos: {:?} to pos: {:?}", item, pos, new_pos);
            println!("Actual state");
            println!("{:?}", self);
            true
        } else {
            // If peer does not exists in hashmap, push it to the first queue
            let new_mp = if manycall_first_use { 1 } else { 0 };
            self.queues_vector[1].push(item.clone());
            self.map.insert(item.clone(), PeerData{vector_position: 1, manycall_in_progress: new_mp});
            println!("Pushed new peer: {:?} to pos: {:?}", item, 1);
            println!("Actual state");
            println!("{:?}", self);
            true
        }

    }

    pub fn pop_req(&mut self, item: &PeerId, is_manycall: bool) -> bool {
        // Check if peer exists in hashmap
        if let Some(data) = self.map.get(item) {
            // If exists delete from actual position in vector of queues
            let pos = data.vector_position;
            let mp = data.manycall_in_progress;

            self.queues_vector[pos].retain(|x| x != item);            

            let new_pos = if pos == 0 {
                pos
            } else {
                pos - 1
            };

            // Check if new position is different from actual position (when actual is 0)
            if new_pos > 0 || ( new_pos == 0 && is_manycall ) || ( new_pos == 0 && !is_manycall && mp > 0 ) {
                self.queues_vector[new_pos].push(item.clone());
                self.map.insert(item.clone(), PeerData{vector_position: new_pos, manycall_in_progress: mp});
                println!("Popped: {:?} from pos: {:?} to pos: {:?}", item, pos, new_pos);
            }
            else {
                self.map.remove(item);
                println!("Removed: {:?} from pos: {:?}", item, pos);
            }
            while self.queues_vector.len() > 2 && self.queues_vector.last().map_or(false, |deque| deque.is_empty()) {
                self.queues_vector.pop();
            }
            println!("Actual state");
            println!("{:?}", self);
            true
        } else {
            false
        }
    }

    pub fn get_peer(&mut self, providers: &HashSet<PeerId>) -> Option<PeerId> {
        for (index, queue) in self.queues_vector.iter().enumerate() {
            for item in queue {
                if providers.contains(item) {
                    println!("Requested peer...");
                    println!("Actual state");
                    println!("{:?}", self);
                    println!("First peer from providers found: {:?}", item);
                    let provider = item.clone();
                    self.queues_vector[index].retain(|x| x != &provider);
                    return Some(provider);
                }
            }
        }
        None
    }

    pub fn remove_manycall(&mut self, providers: &HashSet<PeerId>) {
        for provider in providers {
            if let Some(data) = self.map.get(provider) {
                let pos = data.vector_position;
                let mp = data.manycall_in_progress;
                if mp == 1 {
                    self.queues_vector[pos].retain(|x| x != provider);
                    self.map.remove(provider);
                    println!("Removed provider {:?} from position {:?}", provider, pos);
                }
                else {
                    self.map.insert(provider.clone(), PeerData{vector_position: pos, manycall_in_progress: mp - 1});
                    println!("Decreased manycall count for provider {:?} from position {:?}", provider, pos);
                }
            }
        }
        println!("Actual state");
        println!("{:?}", self);
    }
}
