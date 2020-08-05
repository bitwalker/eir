use std::cmp::Eq;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use cranelift_bforest::{BoundSet, Set, SetForest};
use cranelift_entity::packed_option::ReservedValue;
use cranelift_entity::{entity_impl, EntityList, ListPool, PrimaryMap};

use libeir_util_datastructures::aux_traits::{AuxDebug, AuxEq, AuxHash, HasAux};
use libeir_util_datastructures::dedup_aux_primary_map::DedupAuxPrimaryMap;

use libeir_diagnostics::SourceSpan;

use crate::constant::{Const, ConstKind, ConstantContainer};
use crate::{ArcDialect, FunctionIdent};

pub mod builder;
use builder::IntoValue;

mod pool_container;
use pool_container::PoolContainer;

mod op;
pub use op::{BasicType, CallKind, MapPutUpdate, MatchKind, OpKind};

mod primop;
pub use primop::{BinOp, LogicOp, PrimOpKind};

mod value;
use value::ValueMap;
pub use value::{Value, ValueKind};

mod location;
pub use location::{Location, LocationContainer};

mod format;
pub use format::{ContainerDebug, ContainerDebugAdapter};

//mod serialize;

/// Block/continuation
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Block(u32);
entity_impl!(Block, "block");
impl Default for Block {
    fn default() -> Self {
        Block::reserved_value()
    }
}
impl<C> AuxDebug<C> for Block {
    fn aux_fmt(&self, f: &mut std::fmt::Formatter<'_>, _aux: &C) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
pub struct Argument(u32);
entity_impl!(Argument, "argument");

/// Reference to other function
#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct FunRef(u32);
entity_impl!(FunRef, "fun_ref");

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrimOp(u32);
entity_impl!(PrimOp, "prim_op");

#[derive(Clone)]
pub struct BlockData {
    pub(crate) arguments: EntityList<Value>,

    pub(crate) op: Option<OpKind>,
    pub(crate) reads: EntityList<Value>,

    pub(crate) location: Location,

    // Auxilary data for graph implementation

    // These will contain all the connected blocks, regardless
    // of whether they are actually alive or not.
    pub(crate) predecessors: Set<Block>,
    pub(crate) successors: Set<Block>,
}

#[derive(Debug, Clone)]
pub struct PrimOpData {
    op: PrimOpKind,
    reads: EntityList<Value>,
}
impl AuxHash<PoolContainer> for PrimOpData {
    fn aux_hash<H: Hasher>(&self, state: &mut H, container: &PoolContainer) {
        self.op.hash(state);
        self.reads.as_slice(&container.value).hash(state);
    }
}
impl AuxEq<PoolContainer> for PrimOpData {
    fn aux_eq(
        &self,
        rhs: &PrimOpData,
        self_aux: &PoolContainer,
        other_aux: &PoolContainer,
    ) -> bool {
        if self.op != rhs.op {
            return false;
        }
        self.reads.as_slice(&self_aux.value) == rhs.reads.as_slice(&other_aux.value)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum AttributeKey {
    Continuation,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    None,
}

#[derive(Clone)]
pub struct Function {
    // Meta
    ident: FunctionIdent,
    entry_block: Option<Block>,
    span: SourceSpan,

    dialect: ArcDialect,

    pub(crate) blocks: PrimaryMap<Block, BlockData>,
    pub(crate) values: ValueMap,
    pub(crate) primops: DedupAuxPrimaryMap<PrimOp, PrimOpData, PoolContainer>,

    pub pool: PoolContainer,

    constant_container: ConstantContainer,

    // Auxiliary information
    pub constant_values: HashSet<Value>,
    pub locations: LocationContainer,
}

impl Function {
    pub fn dialect(&self) -> &ArcDialect {
        &self.dialect
    }

    pub fn span(&self) -> SourceSpan {
        self.span
    }

    pub fn cons(&self) -> &ConstantContainer {
        &self.constant_container
    }
}

impl HasAux<ListPool<Value>> for Function {
    fn get_aux(&self) -> &ListPool<Value> {
        &self.pool.value
    }
}
impl HasAux<SetForest<Block>> for Function {
    fn get_aux(&self) -> &SetForest<Block> {
        &self.pool.block_set
    }
}

impl<C: HasAux<Function>> AuxDebug<C> for Function {
    fn aux_fmt(&self, _f: &mut std::fmt::Formatter<'_>, _container: &C) -> std::fmt::Result {
        unimplemented!()
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.aux_fmt(fmt, self)
    }
}

/// Values
impl Function {
    pub fn value_get<T>(&self, v: T) -> Option<Value>
    where
        T: IntoValue,
    {
        v.get_value(self)
    }

    pub fn iter_constants(&self) -> std::collections::hash_set::Iter<'_, Value> {
        self.constant_values.iter()
    }

    pub fn const_kind(&self, constant: Const) -> &ConstKind {
        self.constant_container.const_kind(constant)
    }

    pub fn const_entries<'f>(&'f self, entries: &'f EntityList<Const>) -> &'f [Const] {
        entries.as_slice(&self.constant_container.const_pool)
    }

    pub fn value_kind(&self, value: Value) -> ValueKind {
        self.values[value].kind
    }

    pub fn value_locations(&self, value: Value) -> Option<Vec<SourceSpan>> {
        self.values[value]
            .location
            .as_ref()
            .map(|loc| self.locations.lookup(loc))
    }

    pub fn value_is_constant(&self, value: Value) -> bool {
        self.constant_values.contains(&value)
    }

    pub fn value_list_length(&self, value: Value) -> usize {
        match self.value_kind(value) {
            ValueKind::PrimOp(prim) => {
                if let PrimOpKind::ValueList = self.primop_kind(prim) {
                    return self.primop_reads(prim).len();
                }
            }
            _ => (),
        }
        1
    }

    pub fn value_list_get_n(&self, value: Value, n: usize) -> Option<Value> {
        match self.value_kind(value) {
            ValueKind::PrimOp(prim) => {
                if let PrimOpKind::ValueList = self.primop_kind(prim) {
                    let reads = self.primop_reads(prim);
                    return reads.get(n).cloned();
                }
            }
            _ => (),
        }

        if n == 0 {
            Some(value)
        } else {
            None
        }
    }

    /// If the value is a variable, get its definition block and argument position
    pub fn value_argument(&self, value: Value) -> Option<(Block, usize)> {
        if let ValueKind::Argument(block, arg) = self.values[value].kind {
            Some((block, arg))
        } else {
            None
        }
    }

    pub fn value_block(&self, value: Value) -> Option<Block> {
        if let ValueKind::Block(block) = self.values[value].kind {
            Some(block)
        } else {
            None
        }
    }

    pub fn value_const(&self, value: Value) -> Option<Const> {
        if let ValueKind::Const(con) = &self.values[value].kind {
            Some(*con)
        } else {
            None
        }
    }

    pub fn value_primop(&self, value: Value) -> Option<PrimOp> {
        if let ValueKind::PrimOp(prim) = &self.values[value].kind {
            Some(*prim)
        } else {
            None
        }
    }

    pub fn value_usages(&self, value: Value) -> BoundSet<Block, ()> {
        self.values[value].usages.bind(&self.pool.block_set, &())
    }

    /// Walks all nested values contained within
    /// the tree of potential PrimOps.
    pub fn value_walk_nested_values<F, R>(&self, value: Value, visit: &mut F) -> Result<(), R>
    where
        F: FnMut(Value) -> Result<(), R>,
    {
        visit(value)?;
        if let ValueKind::PrimOp(primop) = self.values[value].kind {
            self.primop_walk_nested_values(primop, visit)?;
        }
        Ok(())
    }
    pub fn value_walk_nested_values_mut<F, R>(
        &mut self,
        value: Value,
        visit: &mut F,
    ) -> Result<(), R>
    where
        F: FnMut(&mut Function, Value) -> Result<(), R>,
    {
        visit(self, value)?;
        if let ValueKind::PrimOp(primop) = self.values[value].kind {
            self.primop_walk_nested_values_mut(primop, visit)?;
        }
        Ok(())
    }
}

/// PrimOps
impl Function {
    pub fn primop_kind(&self, primop: PrimOp) -> &PrimOpKind {
        &self.primops[primop].op
    }
    pub fn primop_reads(&self, primop: PrimOp) -> &[Value] {
        &self.primops[primop].reads.as_slice(&self.pool.value)
    }

    pub fn primop_walk_nested_values<F, R>(&self, primop: PrimOp, visit: &mut F) -> Result<(), R>
    where
        F: FnMut(Value) -> Result<(), R>,
    {
        let data = &self.primops[primop];
        for read in data.reads.as_slice(&self.pool.value) {
            self.value_walk_nested_values(*read, visit)?;
        }
        Ok(())
    }

    pub fn primop_walk_nested_values_mut<F, R>(
        &mut self,
        primop: PrimOp,
        visit: &mut F,
    ) -> Result<(), R>
    where
        F: FnMut(&mut Function, Value) -> Result<(), R>,
    {
        let len = self.primops[primop].reads.as_slice(&self.pool.value).len();
        for n in 0..len {
            let read = self.primops[primop].reads.as_slice(&self.pool.value)[n];
            self.value_walk_nested_values_mut(read, visit)?;
        }
        Ok(())
    }
}

/// Blocks
impl Function {
    #[inline(always)]
    fn block_insert(&mut self) -> Block {
        self.block_insert_with_span(None)
    }

    fn block_insert_with_span(&mut self, span: Option<SourceSpan>) -> Block {
        let location = span
            .map(|s| self.locations.location(None, None, None, None, s))
            .unwrap_or_else(|| self.locations.location_empty());
        let block = self.blocks.push(BlockData {
            arguments: EntityList::new(),

            op: None,
            reads: EntityList::new(),

            predecessors: Set::new(),
            successors: Set::new(),

            location,
        });
        self.values.push(ValueKind::Block(block));
        block
    }

    fn block_arg_insert(&mut self, block: Block) -> Value {
        let arg_num = self.blocks[block].arguments.len(&self.pool.value);
        let val = self.values.push(ValueKind::Argument(block, arg_num));
        self.blocks[block].arguments.push(val, &mut self.pool.value);
        val
    }

    pub fn block_arg_n(&self, block: Block, num: usize) -> Option<Value> {
        self.blocks[block].arguments.get(num, &self.pool.value)
    }

    pub fn block_kind(&self, block: Block) -> Option<&OpKind> {
        self.blocks[block].op.as_ref()
    }

    pub fn block_location(&self, block: Block) -> Location {
        self.blocks[block].location
    }

    pub fn block_locations(&self, block: Block) -> Vec<SourceSpan> {
        let loc = self.blocks[block].location;
        self.locations.lookup(&loc)
    }

    pub fn block_entry(&self) -> Block {
        self.entry_block.expect("Entry block not set on function")
    }
    pub fn block_args<B>(&self, block: B) -> &[Value]
    where
        B: Into<Block>,
    {
        let block: Block = block.into();
        self.blocks[block].arguments.as_slice(&self.pool.value)
    }

    pub fn block_reads(&self, block: Block) -> &[Value] {
        self.blocks[block].reads.as_slice(&self.pool.value)
    }

    pub fn block_value(&self, block: Block) -> Value {
        self.values.get(ValueKind::Block(block)).unwrap()
    }

    pub fn block_walk_nested_values<F, R>(&self, block: Block, visit: &mut F) -> Result<(), R>
    where
        F: FnMut(Value) -> Result<(), R>,
    {
        let reads_len = self.blocks[block].reads.as_slice(&self.pool.value).len();
        for n in 0..reads_len {
            let read = self.blocks[block].reads.get(n, &self.pool.value).unwrap();
            self.value_walk_nested_values(read, visit)?;
        }
        Ok(())
    }
    pub fn block_walk_nested_values_mut<F, R>(
        &mut self,
        block: Block,
        visit: &mut F,
    ) -> Result<(), R>
    where
        F: FnMut(&mut Function, Value) -> Result<(), R>,
    {
        let reads_len = self.blocks[block].reads.as_slice(&self.pool.value).len();
        for n in 0..reads_len {
            let read = self.blocks[block].reads.get(n, &self.pool.value).unwrap();
            self.value_walk_nested_values_mut(read, visit)?;
        }
        Ok(())
    }

    pub fn block_op_eq(&self, lb: Block, r_fun: &Function, rb: Block) -> bool {
        match (self.block_kind(lb).unwrap(), r_fun.block_kind(rb).unwrap()) {
            (OpKind::Call(l), OpKind::Call(r)) => l == r,
            (OpKind::IfBool, OpKind::IfBool) => true,
            (OpKind::Dyn(l), OpKind::Dyn(r)) => l.op_eq(&**r),
            (OpKind::TraceCaptureRaw, OpKind::TraceCaptureRaw) => true,
            (OpKind::TraceConstruct, OpKind::TraceConstruct) => true,
            (OpKind::MapPut { action: a1 }, OpKind::MapPut { action: a2 }) if a1 == a2 => true,
            (OpKind::UnpackValueList(n1), OpKind::UnpackValueList(n2)) if n1 == n2 => true,
            (OpKind::Match { branches: b1 }, OpKind::Match { branches: b2 }) if b1 == b2 => true,
            (OpKind::Unreachable, OpKind::Unreachable) => true,
            _ => false,
        }
    }

    // Iterates through ALL blocks in the function container
    pub fn block_iter(&self) -> impl Iterator<Item = Block> {
        self.blocks.keys()
    }
}

/// Graph
impl Function {
    /// Validates graph invariants for the block.
    /// Relatively inexpensive, for debug assertions.
    pub(crate) fn graph_validate_block(&self, block: Block) {
        let block_data = &self.blocks[block];

        let mut successors_set = HashSet::new();
        self.block_walk_nested_values::<_, ()>(block, &mut |val| {
            if let ValueKind::Block(succ_block) = self.value_kind(val) {
                assert!(block_data
                    .successors
                    .contains(succ_block, &self.pool.block_set, &()));
                assert!(self.blocks[succ_block].predecessors.contains(
                    block,
                    &self.pool.block_set,
                    &()
                ));
                successors_set.insert(succ_block);
            }
            Ok(())
        })
        .unwrap();

        assert!(block_data.successors.iter(&self.pool.block_set).count() == successors_set.len());
    }

    /// Validates graph invariants globally, for the whole
    /// function.
    /// Relatively expensive. Should only be used in tests.
    pub fn graph_validate_global(&self) {
        for block in self.blocks.keys() {
            self.graph_validate_block(block);
        }
    }
}

pub trait GeneralSet<V> {
    fn contains(&self, key: &V, fun: &Function) -> bool;
    fn insert(&mut self, key: V, fun: &mut Function) -> bool;
}
impl<V> GeneralSet<V> for HashSet<V>
where
    V: Hash + Eq,
{
    fn contains(&self, key: &V, _fun: &Function) -> bool {
        HashSet::contains(self, key)
    }
    fn insert(&mut self, key: V, _fun: &mut Function) -> bool {
        HashSet::insert(self, key)
    }
}
impl<V> GeneralSet<V> for Set<V>
where
    V: Copy + Ord + SetPoolProvider,
{
    fn contains(&self, key: &V, fun: &Function) -> bool {
        Set::contains(self, *key, V::pool(fun), &())
    }
    fn insert(&mut self, key: V, fun: &mut Function) -> bool {
        Set::insert(self, key, V::pool_mut(fun), &())
    }
}

pub trait SetPoolProvider: Sized + Copy {
    fn pool(fun: &Function) -> &SetForest<Self>;
    fn pool_mut(fun: &mut Function) -> &mut SetForest<Self>;
}
impl SetPoolProvider for Block {
    fn pool(fun: &Function) -> &SetForest<Block> {
        &fun.pool.block_set
    }
    fn pool_mut(fun: &mut Function) -> &mut SetForest<Block> {
        &mut fun.pool.block_set
    }
}

impl Function {
    pub fn new(span: SourceSpan, ident: FunctionIdent) -> Self {
        Function {
            ident,
            span,

            dialect: crate::dialect::NORMAL.clone(),

            blocks: PrimaryMap::new(),
            values: ValueMap::new(),
            primops: DedupAuxPrimaryMap::new(),

            entry_block: None,

            pool: PoolContainer {
                value: ListPool::new(),
                block_set: SetForest::new(),
            },

            constant_container: ConstantContainer::new(),

            constant_values: HashSet::new(),

            locations: LocationContainer::new(),
        }
    }

    pub fn ident(&self) -> &FunctionIdent {
        &self.ident
    }

    pub fn entry_arg_num(&self) -> usize {
        self.block_args(self.block_entry()).len()
    }
}
