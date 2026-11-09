//! Minimal ECS: generational entities, sparse component storages and
//! singleton resources. Storages and resources live in separate `RefCell`s,
//! so a system can hold several of them at once (e.g. read `Pos` while
//! writing `Health`). Borrowing the same storage mutably twice panics.

use std::any::{type_name, Any, TypeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Entity {
    index: u32,
    generation: u32,
}

pub struct Storage<T> {
    slots: Vec<Option<(u32, T)>>,
}

impl<T> Storage<T> {
    fn new() -> Self {
        Self { slots: Vec::new() }
    }

    pub fn insert(&mut self, e: Entity, value: T) {
        let i = e.index as usize;
        if self.slots.len() <= i {
            self.slots.resize_with(i + 1, || None);
        }
        self.slots[i] = Some((e.generation, value));
    }

    pub fn remove(&mut self, e: Entity) -> Option<T> {
        let i = e.index as usize;
        match self.slots.get(i) {
            Some(Some((g, _))) if *g == e.generation => self.slots[i].take().map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get(&self, e: Entity) -> Option<&T> {
        match self.slots.get(e.index as usize) {
            Some(Some((g, v))) if *g == e.generation => Some(v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        match self.slots.get_mut(e.index as usize) {
            Some(Some((g, v))) if *g == e.generation => Some(v),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> + '_ {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.as_ref().map(|(g, v)| (Entity { index: i as u32, generation: *g }, v))
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> + '_ {
        self.slots.iter_mut().enumerate().filter_map(|(i, s)| {
            s.as_mut().map(|(g, v)| (Entity { index: i as u32, generation: *g }, v))
        })
    }
}

trait AnyStorage {
    fn remove_entity(&mut self, e: Entity);
    fn clear(&mut self);
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> AnyStorage for Storage<T> {
    fn remove_entity(&mut self, e: Entity) {
        self.remove(e);
    }
    fn clear(&mut self) {
        self.slots.clear();
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default)]
struct Allocator {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
}

impl Allocator {
    fn kill(&mut self, index: u32) {
        let i = index as usize;
        self.alive[i] = false;
        self.generations[i] = self.generations[i].wrapping_add(1);
        self.free.push(index);
    }
}

pub struct World {
    alloc: RefCell<Allocator>,
    storages: HashMap<TypeId, RefCell<Box<dyn AnyStorage>>>,
    resources: HashMap<TypeId, RefCell<Box<dyn Any>>>,
    despawn_queue: RefCell<Vec<Entity>>,
}

impl World {
    pub fn new() -> Self {
        Self {
            alloc: RefCell::new(Allocator::default()),
            storages: HashMap::new(),
            resources: HashMap::new(),
            despawn_queue: RefCell::new(Vec::new()),
        }
    }

    pub fn register<T: 'static>(&mut self) {
        let prev = self
            .storages
            .insert(TypeId::of::<T>(), RefCell::new(Box::new(Storage::<T>::new())));
        assert!(prev.is_none(), "component {} registered twice", type_name::<T>());
    }

    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        let prev = self.resources.insert(TypeId::of::<T>(), RefCell::new(Box::new(value)));
        assert!(prev.is_none(), "resource {} inserted twice", type_name::<T>());
    }

    fn resource_cell<T: 'static>(&self) -> &RefCell<Box<dyn Any>> {
        self.resources
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("resource {} is not inserted", type_name::<T>()))
    }

    fn storage_cell<T: 'static>(&self) -> &RefCell<Box<dyn AnyStorage>> {
        self.storages
            .get(&TypeId::of::<T>())
            .unwrap_or_else(|| panic!("component {} is not registered", type_name::<T>()))
    }

    pub fn res<T: 'static>(&self) -> Ref<'_, T> {
        Ref::map(self.resource_cell::<T>().borrow(), |b| {
            b.downcast_ref::<T>().expect("resource type mismatch")
        })
    }

    pub fn res_mut<T: 'static>(&self) -> RefMut<'_, T> {
        RefMut::map(self.resource_cell::<T>().borrow_mut(), |b| {
            b.downcast_mut::<T>().expect("resource type mismatch")
        })
    }

    pub fn read<T: 'static>(&self) -> Ref<'_, Storage<T>> {
        Ref::map(self.storage_cell::<T>().borrow(), |s| {
            s.as_any().downcast_ref::<Storage<T>>().expect("storage type mismatch")
        })
    }

    pub fn write<T: 'static>(&self) -> RefMut<'_, Storage<T>> {
        RefMut::map(self.storage_cell::<T>().borrow_mut(), |s| {
            s.as_any_mut().downcast_mut::<Storage<T>>().expect("storage type mismatch")
        })
    }

    pub fn spawn(&self) -> Entity {
        let mut a = self.alloc.borrow_mut();
        if let Some(index) = a.free.pop() {
            a.alive[index as usize] = true;
            Entity { index, generation: a.generations[index as usize] }
        } else {
            let index = a.generations.len() as u32;
            a.generations.push(0);
            a.alive.push(true);
            Entity { index, generation: 0 }
        }
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        let a = self.alloc.borrow();
        let i = e.index as usize;
        i < a.alive.len() && a.alive[i] && a.generations[i] == e.generation
    }

    pub fn insert<T: 'static>(&self, e: Entity, component: T) {
        assert!(self.is_alive(e), "insert {} into dead entity {:?}", type_name::<T>(), e);
        self.write::<T>().insert(e, component);
    }

    /// Deferred: the entity is removed on the next `maintain()`.
    pub fn despawn(&self, e: Entity) {
        self.despawn_queue.borrow_mut().push(e);
    }

    /// Applies queued despawns. Must be called while no storage is borrowed.
    pub fn maintain(&self) {
        let queue: Vec<Entity> = std::mem::take(&mut *self.despawn_queue.borrow_mut());
        for e in queue {
            // An entity may be queued twice in one tick; the second request is stale.
            if !self.is_alive(e) {
                continue;
            }
            for cell in self.storages.values() {
                cell.borrow_mut().remove_entity(e);
            }
            self.alloc.borrow_mut().kill(e.index);
        }
    }

    pub fn clear_entities(&self) {
        self.despawn_queue.borrow_mut().clear();
        for cell in self.storages.values() {
            cell.borrow_mut().clear();
        }
        let mut a = self.alloc.borrow_mut();
        for i in 0..a.alive.len() {
            if a.alive[i] {
                a.kill(i as u32);
            }
        }
    }
}
