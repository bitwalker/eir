use libeir_ir::{
    constant::EmptyMap, Block as IrBlock, FunctionBuilder, MapPutUpdate, Value as IrValue,
};

use libeir_intern::Symbol;

use crate::lower::{lower_single, LowerCtx};
use crate::parser::ast::{Map, MapField, MapUpdate};

pub(super) fn lower_map_update_expr(
    ctx: &mut LowerCtx,
    b: &mut FunctionBuilder,
    mut block: IrBlock,
    map: &MapUpdate,
) -> (IrBlock, IrValue) {
    let entry_map_val = map_block!(block, lower_single(ctx, b, block, &map.map));
    let mut map_builder = b.op_map_put_build(map.span, entry_map_val);

    for field in map.updates.iter() {
        let (key, value, action) = match field {
            MapField::Assoc { key, value, .. } => (key, value, MapPutUpdate::Put),
            MapField::Exact { key, value, .. } => (key, value, MapPutUpdate::Update),
        };

        let key_val = map_block!(block, lower_single(ctx, b, block, key));
        let value_val = map_block!(block, lower_single(ctx, b, block, value));
        map_builder.push_kv(key_val, value_val, action, b);
    }

    let loc = ctx.current_location(b, map.span);
    b.block_set_location(block, loc);
    let (ok, fail) = map_builder.finish(block, b);

    let typ_val = b.value(Symbol::intern("error"));
    let badmatch_val = b.value(Symbol::intern("badkey"));
    let failed_key = b.block_args(fail)[0];
    let err_val = b.prim_tuple(map.span, &[badmatch_val, failed_key]);
    ctx.exc_stack
        .make_error_jump(b, map.span, fail, typ_val, err_val);

    (ok, b.block_args(ok)[0])
}

pub(super) fn lower_map_expr(
    ctx: &mut LowerCtx,
    b: &mut FunctionBuilder,
    mut block: IrBlock,
    map: &Map,
) -> (IrBlock, IrValue) {
    let empty_map = b.value(EmptyMap);
    let mut map_builder = b.op_map_put_build(map.span, empty_map);

    for field in map.fields.iter() {
        let (key, value, action) = match field {
            MapField::Assoc { key, value, .. } => (key, value, MapPutUpdate::Put),
            MapField::Exact { key, value, .. } => (key, value, MapPutUpdate::Update),
        };

        let key_val = map_block!(block, lower_single(ctx, b, block, key));
        let value_val = map_block!(block, lower_single(ctx, b, block, value));
        map_builder.push_kv(key_val, value_val, action, b);
    }

    let loc = ctx.current_location(b, map.span);
    b.block_set_location(block, loc);
    let (ok, fail) = map_builder.finish(block, b);

    let typ_val = b.value(Symbol::intern("error"));
    let badmatch_val = b.value(Symbol::intern("badkey"));
    let failed_key = b.block_args(fail)[0];
    let err_val = b.prim_tuple(map.span, &[badmatch_val, failed_key]);
    ctx.exc_stack
        .make_error_jump(b, map.span, fail, typ_val, err_val);

    (ok, b.block_args(ok)[0])
}
