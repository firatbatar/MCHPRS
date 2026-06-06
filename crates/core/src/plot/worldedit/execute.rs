use super::*;
use crate::config::CONFIG;
use crate::player::PacketSender;
use crate::plot::PLOT_BLOCK_HEIGHT;
use crate::utils::{self, HyphenatedUUID};
use mchprs_blocks::block_entities::InventoryEntry;
use mchprs_blocks::blocks::{Block, FlipDirection, HopperFacing, RotateAmt};
use mchprs_blocks::items::{Item, ItemStack};
use mchprs_blocks::{BlockDirection, BlockFace, BlockFacing, BlockPos};
use mchprs_network::packets::clientbound::*;
use mchprs_schematic::{
    load_schematic, load_schematic_from_reader, paste_clipboard, paste_clipboard_batch,
    save_schematic, WorldEditClipboard,
};
use mchprs_text::{ColorCode, TextComponentBuilder};
use mchprs_world::World;
use std::path::PathBuf;
use std::time::Instant;
use tracing::error;

pub(super) fn execute_wand(ctx: CommandExecuteContext<'_>) {
    let item = ItemStack {
        count: 1,
        item_type: Item::WoodenAxe,
        nbt: None,
    };
    ctx.player.inventory[(ctx.player.selected_slot + 36) as usize] = Some(item);
    let entity_equipment = CSetEquipment {
        entity_id: ctx.player.entity_id as i32,
        equipment: vec![CSetEquipmentEquipment {
            slot: 0,
            item: ctx.player.inventory[(ctx.player.selected_slot + 36) as usize]
                .as_ref()
                .map(utils::encode_slot_data),
        }],
    }
    .encode();
    for player in &mut ctx.plot.packet_senders {
        player.send_packet(&entity_equipment);
    }
}

pub(super) fn execute_set(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();
    let pattern = ctx.arguments[0].unwrap_pattern();

    let mut operation = worldedit_start_operation(ctx.player);
    capture_undo(
        ctx.plot,
        ctx.player,
        ctx.player.first_position.unwrap(),
        ctx.player.second_position.unwrap(),
    );
    for x in operation.x_range() {
        for y in operation.y_range() {
            for z in operation.z_range() {
                let block_pos = BlockPos::new(x, y, z);
                let block_id = pattern.pick().get_id();

                if ctx.plot.set_block_raw(block_pos, block_id) {
                    operation.update_block();
                }
            }
        }
    }

    let blocks_updated = operation.blocks_updated();

    ctx.player.send_worldedit_message(&format!(
        "Operation completed: {} block(s) affected ({:?})",
        blocks_updated,
        start_time.elapsed()
    ));
}

pub(super) fn execute_replace(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let filter = ctx.arguments[0].unwrap_mask();
    let pattern = ctx.arguments[1].unwrap_pattern();

    let mut operation = worldedit_start_operation(ctx.player);
    capture_undo(
        ctx.plot,
        ctx.player,
        ctx.player.first_position.unwrap(),
        ctx.player.second_position.unwrap(),
    );
    for x in operation.x_range() {
        for y in operation.y_range() {
            for z in operation.z_range() {
                let block_pos = BlockPos::new(x, y, z);

                if filter.matches(ctx.plot.get_block(block_pos)) {
                    let block_id = pattern.pick().get_id();

                    if ctx.plot.set_block_raw(block_pos, block_id) {
                        operation.update_block();
                    }
                }
            }
        }
    }

    let blocks_updated = operation.blocks_updated();

    ctx.player.send_worldedit_message(&format!(
        "Operation completed: {} block(s) affected ({:?})",
        blocks_updated,
        start_time.elapsed()
    ));
}

pub(super) fn execute_count(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let filter = ctx.arguments[0].unwrap_mask();

    let mut blocks_counted = 0;
    let operation = worldedit_start_operation(ctx.player);
    for x in operation.x_range() {
        for y in operation.y_range() {
            for z in operation.z_range() {
                let block_pos = BlockPos::new(x, y, z);
                if filter.matches(ctx.plot.get_block(block_pos)) {
                    blocks_counted += 1;
                }
            }
        }
    }

    ctx.player.send_worldedit_message(&format!(
        "Counted {} block(s) ({:?})",
        blocks_counted,
        start_time.elapsed()
    ));
}

pub(super) fn execute_copy(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let origin = ctx.player.pos.block_pos();
    let clipboard = create_clipboard(
        ctx.plot,
        origin,
        ctx.player.first_position.unwrap(),
        ctx.player.second_position.unwrap(),
    );
    ctx.player.worldedit_clipboard = Some(clipboard);

    ctx.player.send_worldedit_message(&format!(
        "Your selection was copied. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_cut(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let first_pos = ctx.player.first_position.unwrap();
    let second_pos = ctx.player.second_position.unwrap();

    capture_undo(ctx.plot, ctx.player, first_pos, second_pos);

    let origin = ctx.player.pos.block_pos();
    let clipboard = create_clipboard(ctx.plot, origin, first_pos, second_pos);
    ctx.player.worldedit_clipboard = Some(clipboard);
    clear_area(ctx.plot, first_pos, second_pos);

    ctx.player.send_worldedit_message(&format!(
        "Your selection was cut. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_move(mut ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let move_amt = ctx.arguments[0].unwrap_uint();
    let direction = ctx.arguments[1].unwrap_direction();

    let first_pos = ctx.player.first_position.unwrap();
    let second_pos = ctx.player.second_position.unwrap();

    let zero_pos = BlockPos::new(0, 0, 0);

    let undo = WorldEditUndo {
        clipboards: vec![
            create_clipboard(ctx.plot, first_pos.min(second_pos), first_pos, second_pos),
            create_clipboard(
                ctx.plot,
                first_pos.min(second_pos),
                direction.offset_pos(first_pos, move_amt as i32),
                direction.offset_pos(second_pos, move_amt as i32),
            ),
        ],
        pos: first_pos.min(second_pos),
        plot_x: ctx.plot.x,
        plot_z: ctx.plot.z,
    };
    ctx.player.worldedit_undo.push(undo);

    let clipboard = create_clipboard(ctx.plot, zero_pos, first_pos, second_pos);
    clear_area(ctx.plot, first_pos, second_pos);
    paste_clipboard(
        ctx.plot,
        &clipboard,
        direction.offset_pos(zero_pos, move_amt as i32),
        ctx.has_flag('a'),
    );

    if ctx.has_flag('s') {
        let first_pos = direction.offset_pos(first_pos, move_amt as i32);
        let second_pos = direction.offset_pos(second_pos, move_amt as i32);
        let player = &mut ctx.player;
        player.worldedit_set_first_position(first_pos);
        player.worldedit_set_second_position(second_pos);
    }

    ctx.player.send_worldedit_message(&format!(
        "Your selection was moved. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_paste(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    if ctx.player.worldedit_clipboard.is_some() {
        // Here I am cloning the clipboard. This is bad. Don't do this.
        let cb = &ctx.player.worldedit_clipboard.clone().unwrap();
        let pos = ctx.player.pos.block_pos();
        let offset_x = pos.x - cb.offset_x;
        let offset_y = pos.y - cb.offset_y;
        let offset_z = pos.z - cb.offset_z;
        let first_pos = BlockPos::new(offset_x, offset_y, offset_z);
        let second_pos = BlockPos::new(
            offset_x + cb.size_x as i32 - 1,
            offset_y + cb.size_y as i32 - 1,
            offset_z + cb.size_z as i32 - 1,
        );
        capture_undo(ctx.plot, ctx.player, first_pos, second_pos);
        paste_clipboard(ctx.plot, cb, pos, ctx.has_flag('a'));
        if ctx.has_flag('u') {
            update(ctx.plot, first_pos, second_pos);
        }
        if ctx.has_flag('s') {
            ctx.player.worldedit_set_first_position(first_pos);
            ctx.player.worldedit_set_second_position(second_pos);
        }
        ctx.player.send_worldedit_message(&format!(
            "Your clipboard was pasted. ({:?})",
            start_time.elapsed()
        ));
    } else {
        ctx.player.send_system_message("Your clipboard is empty!");
    }
}

static SCHEMATI_VALIDATE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z0-9_.]+\.schem(atic)?").unwrap());

pub(super) fn execute_load(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let mut file_name = ctx.arguments[0].unwrap_string().clone();
    if !SCHEMATI_VALIDATE_REGEX.is_match(&file_name) {
        ctx.player.send_error_message("Filename is invalid");
        return;
    }

    if CONFIG.schemati {
        let prefix = HyphenatedUUID(ctx.player.uuid).to_string() + "/";
        file_name.insert_str(0, &prefix);
    }

    let path = PathBuf::from("./schems").join(file_name);
    let clipboard = load_schematic(&path);
    match clipboard {
        Ok(cb) => {
            ctx.player.worldedit_clipboard = Some(cb);
            ctx.player.send_worldedit_message(&format!(
                "The schematic was loaded to your clipboard. Do //paste to birth it into the world. ({:?})",
                start_time.elapsed()
            ));
        }
        Err(e) => {
            if let Some(e) = e.downcast_ref::<std::io::Error>()
                && e.kind() == std::io::ErrorKind::NotFound
            {
                let msg = "The specified schematic file could not be found.";
                ctx.player.send_error_message(msg);
                return;
            }
            error!("There was an error loading a schematic:");
            error!("{}", e);
            ctx.player.send_error_message(
                "There was an error loading the schematic. Check console for more details.",
            );
        }
    }
}

pub(super) fn execute_save(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let mut file_name = ctx.arguments[0].unwrap_string().clone();
    if !SCHEMATI_VALIDATE_REGEX.is_match(&file_name) {
        ctx.player.send_error_message("Filename is invalid");
        return;
    }

    if CONFIG.schemati {
        let prefix = HyphenatedUUID(ctx.player.uuid).to_string() + "/";
        file_name.insert_str(0, &prefix);
    }

    let path = PathBuf::from("./schems").join(file_name);
    let clipboard = ctx.player.worldedit_clipboard.as_ref().unwrap();
    match save_schematic(&path, clipboard) {
        Ok(_) => {
            ctx.player.send_worldedit_message(&format!(
                "The schematic was saved sucessfuly. ({:?})",
                start_time.elapsed()
            ));
        }
        Err(err) => {
            error!("There was an error saving a schematic: ");
            error!("{:?}", err);
            ctx.player
                .send_error_message("There was an error saving the schematic.");
        }
    }
}

pub(super) fn execute_stack(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let stack_amt = ctx.arguments[0].unwrap_uint();
    let direction = ctx.arguments[1].unwrap_direction();
    let pos1 = ctx.player.first_position.unwrap();
    let pos2 = ctx.player.second_position.unwrap();
    let clipboard = create_clipboard(ctx.plot, pos1, pos1, pos2);
    let stack_offset = match direction {
        BlockFacing::North | BlockFacing::South => clipboard.size_z,
        BlockFacing::East | BlockFacing::West => clipboard.size_x,
        BlockFacing::Up | BlockFacing::Down => clipboard.size_y,
    };
    let mut undo_cbs = Vec::new();
    for i in 1..stack_amt + 1 {
        let offset = (i * stack_offset) as i32;
        let block_pos = direction.offset_pos(pos1, offset);
        undo_cbs.push(create_clipboard(
            ctx.plot,
            pos1,
            block_pos,
            direction.offset_pos(pos2, offset),
        ));
        paste_clipboard(ctx.plot, &clipboard, block_pos, ctx.has_flag('a'));
    }
    let undo = WorldEditUndo {
        clipboards: undo_cbs,
        pos: pos1,
        plot_x: ctx.plot.x,
        plot_z: ctx.plot.z,
    };
    ctx.player.worldedit_undo.push(undo);

    ctx.player.send_worldedit_message(&format!(
        "Your selection was stacked. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_undo(ctx: CommandExecuteContext<'_>) {
    if ctx.player.worldedit_undo.is_empty() {
        ctx.player
            .send_error_message("There is nothing left to undo.");
        return;
    }
    let undo = ctx.player.worldedit_undo.pop().unwrap();
    if undo.plot_x != ctx.plot.x || undo.plot_z != ctx.plot.z {
        ctx.player
            .send_error_message("Cannot undo outside of your current plot.");
        return;
    }
    let redo = WorldEditUndo {
        clipboards: undo
            .clipboards
            .iter()
            .map(|clipboard| {
                let first_pos = BlockPos {
                    x: undo.pos.x - clipboard.offset_x,
                    y: undo.pos.y - clipboard.offset_y,
                    z: undo.pos.z - clipboard.offset_z,
                };
                let second_pos = BlockPos {
                    x: first_pos.x + clipboard.size_x as i32 - 1,
                    y: first_pos.y + clipboard.size_y as i32 - 1,
                    z: first_pos.z + clipboard.size_z as i32 - 1,
                };
                create_clipboard(ctx.plot, undo.pos, first_pos, second_pos)
            })
            .collect(),
        ..undo
    };
    for clipboard in &undo.clipboards {
        paste_clipboard(ctx.plot, clipboard, undo.pos, false);
    }
    ctx.player.worldedit_redo.push(redo);
}

pub(super) fn execute_redo(ctx: CommandExecuteContext<'_>) {
    if ctx.player.worldedit_redo.is_empty() {
        ctx.player
            .send_error_message("There is nothing left to redo.");
        return;
    }
    let redo = ctx.player.worldedit_redo.pop().unwrap();
    if redo.plot_x != ctx.plot.x || redo.plot_z != ctx.plot.z {
        ctx.player
            .send_error_message("Cannot redo outside of your current plot.");
        return;
    }
    let undo = WorldEditUndo {
        clipboards: redo
            .clipboards
            .iter()
            .map(|clipboard| {
                let first_pos = BlockPos {
                    x: redo.pos.x - clipboard.offset_x,
                    y: redo.pos.y - clipboard.offset_y,
                    z: redo.pos.z - clipboard.offset_z,
                };
                let second_pos = BlockPos {
                    x: first_pos.x + clipboard.size_x as i32 - 1,
                    y: first_pos.y + clipboard.size_y as i32 - 1,
                    z: first_pos.z + clipboard.size_z as i32 - 1,
                };
                create_clipboard(ctx.plot, redo.pos, first_pos, second_pos)
            })
            .collect(),
        ..redo
    };
    for clipboard in &redo.clipboards {
        paste_clipboard(ctx.plot, clipboard, redo.pos, false);
    }
    ctx.player.worldedit_undo.push(undo);
}

pub(super) fn execute_sel(ctx: CommandExecuteContext<'_>) {
    let player = ctx.player;
    player.first_position = None;
    player.second_position = None;
    player.send_worldedit_message("Selection cleared.");
    player.worldedit_send_cui("s|cuboid");
}

pub(super) fn execute_pos1(ctx: CommandExecuteContext<'_>) {
    let pos = ctx.player.pos.block_pos();
    ctx.player.worldedit_set_first_position(pos);
}

pub(super) fn execute_pos2(ctx: CommandExecuteContext<'_>) {
    let pos = ctx.player.pos.block_pos();
    ctx.player.worldedit_set_second_position(pos);
}

pub(super) fn execute_hpos1(mut ctx: CommandExecuteContext<'_>) {
    let player = &mut ctx.player;
    let pitch = player.pitch as f64;
    let yaw = player.yaw as f64;

    let result = ray_trace_block(ctx.plot, player.pos, pitch, yaw, 300.0);

    let player = ctx.player;
    match result {
        Some(pos) => player.worldedit_set_first_position(pos),
        None => player.send_error_message("No block in sight!"),
    }
}

pub(super) fn execute_hpos2(mut ctx: CommandExecuteContext<'_>) {
    let player = &mut ctx.player;
    let pitch = player.pitch as f64;
    let yaw = player.yaw as f64;

    let result = ray_trace_block(ctx.plot, player.pos, pitch, yaw, 300.0);

    let player = &mut ctx.player;
    match result {
        Some(pos) => player.worldedit_set_second_position(pos),
        None => player.send_error_message("No block in sight!"),
    }
}

pub(super) fn execute_expand(ctx: CommandExecuteContext<'_>) {
    let amount = ctx.arguments[0].unwrap_uint();
    let direction = ctx.arguments[1].unwrap_direction();
    let player = ctx.player;

    expand_selection(
        player,
        direction.offset_pos(BlockPos::zero(), amount as i32),
        false,
    );

    player.send_worldedit_message(&format!("Region expanded {} block(s).", amount));
}

pub(super) fn execute_contract(ctx: CommandExecuteContext<'_>) {
    let amount = ctx.arguments[0].unwrap_uint();
    let direction = ctx.arguments[1].unwrap_direction();
    let player = ctx.player;

    expand_selection(
        player,
        direction.offset_pos(BlockPos::zero(), amount as i32),
        true,
    );

    player.send_worldedit_message(&format!("Region contracted {} block(s).", amount));
}

pub(super) fn execute_shift(ctx: CommandExecuteContext<'_>) {
    let amount = ctx.arguments[0].unwrap_uint();
    let direction = ctx.arguments[1].unwrap_direction();
    let player = ctx.player;
    let first_pos = player.first_position.unwrap();
    let second_pos = player.second_position.unwrap();

    let mut move_both_points = |x, y, z| {
        player.worldedit_set_first_position(BlockPos::new(
            first_pos.x + x,
            first_pos.y + y,
            first_pos.z + z,
        ));
        player.worldedit_set_second_position(BlockPos::new(
            second_pos.x + x,
            second_pos.y + y,
            second_pos.z + z,
        ));
    };

    match direction {
        BlockFacing::Up => move_both_points(0, amount as i32, 0),
        BlockFacing::Down => move_both_points(0, -(amount as i32), 0),
        BlockFacing::East => move_both_points(amount as i32, 0, 0),
        BlockFacing::West => move_both_points(-(amount as i32), 0, 0),
        BlockFacing::South => move_both_points(0, 0, amount as i32),
        BlockFacing::North => move_both_points(0, 0, -(amount as i32)),
    }

    player.send_worldedit_message(&format!("Region shifted {} block(s).", amount));
}

pub(super) fn execute_flip(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let direction = ctx.arguments[0].unwrap_direction();
    let clipboard = ctx.player.worldedit_clipboard.as_ref().unwrap();
    let size_x = clipboard.size_x;
    let size_y = clipboard.size_y;
    let size_z = clipboard.size_z;
    let volume = size_x * size_y * size_z;

    let flip_pos = |mut pos: BlockPos| {
        match direction {
            BlockFacing::East | BlockFacing::West => pos.x = size_x as i32 - 1 - pos.x,
            BlockFacing::North | BlockFacing::South => pos.z = size_z as i32 - 1 - pos.z,
            BlockFacing::Up | BlockFacing::Down => pos.y = size_y as i32 - 1 - pos.y,
        }
        pos
    };

    let mut newcpdata = PalettedBitBuffer::new((volume) as usize, 9);

    let mut c_x = 0;
    let mut c_y = 0;
    let mut c_z = 0;
    for i in 0..volume {
        let BlockPos {
            x: n_x,
            y: n_y,
            z: n_z,
        } = flip_pos(BlockPos::new(c_x, c_y, c_z));
        let n_i = (n_y as u32 * size_x * size_z) + (n_z as u32 * size_x) + n_x as u32;

        let mut block = Block::from_id(clipboard.data.get_entry(i as usize));
        match direction {
            BlockFacing::East | BlockFacing::West => block.flip(FlipDirection::FlipX),
            BlockFacing::North | BlockFacing::South => block.flip(FlipDirection::FlipZ),
            _ => {}
        }
        newcpdata.set_entry(n_i as usize, block.get_id());

        // Ok now lets increment the coordinates for the next block
        c_x += 1;

        if c_x as u32 == size_x {
            c_x = 0;
            c_z += 1;

            if c_z as u32 == size_z {
                c_z = 0;
                c_y += 1;
            }
        }
    }

    let offset = flip_pos(BlockPos::new(
        clipboard.offset_x,
        clipboard.offset_y,
        clipboard.offset_z,
    ));
    let cb = WorldEditClipboard {
        offset_x: offset.x,
        offset_y: offset.y,
        offset_z: offset.z,
        size_x,
        size_y,
        size_z,
        data: newcpdata,
        block_entities: clipboard
            .block_entities
            .iter()
            .map(|(pos, e)| (flip_pos(*pos), e.clone()))
            .collect(),
    };

    ctx.player.worldedit_clipboard = Some(cb);
    ctx.player.send_worldedit_message(&format!(
        "The clipboard copy has been flipped. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_rotate(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();
    let rotate_amt = ctx.arguments[0].unwrap_uint();
    let rotate_amt = match rotate_amt % 360 {
        0 => {
            ctx.player
                .send_worldedit_message("Successfully rotated by 0! That took a lot of work.");
            return;
        }
        90 => RotateAmt::Rotate90,
        180 => RotateAmt::Rotate180,
        270 => RotateAmt::Rotate270,
        _ => {
            ctx.player
                .send_error_message("Rotate amount must be a multiple of 90.");
            return;
        }
    };

    let clipboard = ctx.player.worldedit_clipboard.as_ref().unwrap();
    let size_x = clipboard.size_x;
    let size_y = clipboard.size_y;
    let size_z = clipboard.size_z;
    let volume = size_x * size_y * size_z;

    let (n_size_x, n_size_z) = match rotate_amt {
        RotateAmt::Rotate90 | RotateAmt::Rotate270 => (size_z, size_x),
        _ => (size_x, size_z),
    };

    let rotate_pos = |pos: BlockPos| match rotate_amt {
        RotateAmt::Rotate90 => BlockPos {
            x: n_size_x as i32 - 1 - pos.z,
            y: pos.y,
            z: pos.x,
        },
        RotateAmt::Rotate180 => BlockPos {
            x: n_size_x as i32 - 1 - pos.x,
            y: pos.y,
            z: n_size_z as i32 - 1 - pos.z,
        },
        RotateAmt::Rotate270 => BlockPos {
            x: pos.z,
            y: pos.y,
            z: n_size_z as i32 - 1 - pos.x,
        },
    };

    let mut newcpdata = PalettedBitBuffer::new((volume) as usize, 9);

    let mut c_x = 0;
    let mut c_y = 0;
    let mut c_z = 0;
    for i in 0..volume {
        let BlockPos {
            x: n_x,
            y: n_y,
            z: n_z,
        } = rotate_pos(BlockPos::new(c_x, c_y, c_z));
        let n_i = (n_y as u32 * n_size_x * n_size_z) + (n_z as u32 * n_size_x) + n_x as u32;

        let mut block = Block::from_id(clipboard.data.get_entry(i as usize));
        block.rotate(rotate_amt);
        newcpdata.set_entry(n_i as usize, block.get_id());

        // Ok now lets increment the coordinates for the next block
        c_x += 1;

        if c_x as u32 == size_x {
            c_x = 0;
            c_z += 1;

            if c_z as u32 == size_z {
                c_z = 0;
                c_y += 1;
            }
        }
    }

    let offset = rotate_pos(BlockPos::new(
        clipboard.offset_x,
        clipboard.offset_y,
        clipboard.offset_z,
    ));
    let cb = WorldEditClipboard {
        offset_x: offset.x,
        offset_y: offset.y,
        offset_z: offset.z,
        size_x: n_size_x,
        size_y,
        size_z: n_size_z,
        data: newcpdata,
        block_entities: clipboard
            .block_entities
            .iter()
            .map(|(pos, e)| (rotate_pos(*pos), e.clone()))
            .collect(),
    };

    ctx.player.worldedit_clipboard = Some(cb);
    ctx.player.send_worldedit_message(&format!(
        "The clipboard copy has been rotated. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_help(mut ctx: CommandExecuteContext<'_>) {
    let command_name = ctx.arguments[0].unwrap_string().clone();
    let slash_command_name = "/".to_owned() + &command_name;
    let player = &mut ctx.player;

    let maybe_command = COMMANDS
        .get(command_name.as_str())
        .map(|c| (command_name.as_str(), c))
        .or_else(|| {
            COMMANDS
                .get(slash_command_name.as_str())
                .map(|c| (slash_command_name.as_str(), c))
        });
    let (command_name, command) = match maybe_command {
        Some(command) => command,
        None => {
            player.send_error_message(&format!("Unknown command: {}", command_name));
            return;
        }
    };

    let mut message = vec![
        TextComponentBuilder::new("--------------".to_owned())
            .color_code(ColorCode::Yellow)
            .strikethrough(true)
            .finish(),
        TextComponentBuilder::new(format!(" Help for /{} ", command_name)).finish(),
        TextComponentBuilder::new("--------------\n".to_owned())
            .color_code(ColorCode::Yellow)
            .strikethrough(true)
            .finish(),
        TextComponentBuilder::new(command.description.to_owned())
            .color_code(ColorCode::Gray)
            .finish(),
        TextComponentBuilder::new("\nUsage: ".to_owned())
            .color_code(ColorCode::Gray)
            .finish(),
        TextComponentBuilder::new(format!("/{}", command_name))
            .color_code(ColorCode::Gold)
            .finish(),
    ];

    for arg in command.arguments {
        message.append(&mut vec![
            TextComponentBuilder::new(" [".to_owned())
                .color_code(ColorCode::Yellow)
                .finish(),
            TextComponentBuilder::new(arg.name.to_owned())
                .color_code(ColorCode::Gold)
                .finish(),
            TextComponentBuilder::new("]".to_owned())
                .color_code(ColorCode::Yellow)
                .finish(),
        ]);
    }

    if !command.arguments.is_empty() {
        message.push(
            TextComponentBuilder::new("\nArguments:".to_owned())
                .color_code(ColorCode::Gray)
                .finish(),
        );
    }

    for arg in command.arguments {
        message.append(&mut vec![
            TextComponentBuilder::new("\n  [".to_owned())
                .color_code(ColorCode::Yellow)
                .finish(),
            TextComponentBuilder::new(arg.name.to_owned())
                .color_code(ColorCode::Gold)
                .finish(),
            TextComponentBuilder::new("]".to_owned())
                .color_code(ColorCode::Yellow)
                .finish(),
        ]);

        let default = if let Some(arg) = &arg.default {
            match arg {
                Argument::UnsignedInteger(int) => Some(int.to_string()),
                _ => None,
            }
        } else {
            match arg.argument_type {
                ArgumentType::Direction | ArgumentType::DirectionVector => Some("me".to_string()),
                ArgumentType::UnsignedInteger => Some("1".to_string()),
                _ => None,
            }
        };
        if let Some(default) = default {
            message.push(
                TextComponentBuilder::new(format!(" (defaults to {})", default))
                    .color_code(ColorCode::Gray)
                    .finish(),
            );
        }

        message.push(
            TextComponentBuilder::new(format!(": {}", arg.description))
                .color_code(ColorCode::Gray)
                .finish(),
        );
    }

    if !command.flags.is_empty() {
        message.push(
            TextComponentBuilder::new("\nFlags:".to_owned())
                .color_code(ColorCode::Gray)
                .finish(),
        );
    }

    for flag in command.flags {
        message.append(&mut vec![
            TextComponentBuilder::new(format!("\n  -{}", flag.letter))
                .color_code(ColorCode::Gold)
                .finish(),
            TextComponentBuilder::new(format!(": {}", flag.description))
                .color_code(ColorCode::Gray)
                .finish(),
        ]);
    }

    player.send_chat_message(&message);
}

pub(super) fn execute_up(ctx: CommandExecuteContext<'_>) {
    let distance = ctx.arguments[0].unwrap_uint();
    let player = ctx.player;

    let mut pos = player.pos;
    pos.y += distance as f64;
    let block_pos = pos.block_pos();

    let platform_pos = block_pos.offset(BlockFace::Bottom);
    if matches!(ctx.plot.get_block(platform_pos), Block::Air) {
        ctx.plot.set_block(platform_pos, Block::Glass {});
    }

    player.teleport(pos);
}

pub(super) fn execute_ascend(ctx: CommandExecuteContext<'_>) {
    let initial_levels = ctx.arguments[0].unwrap_uint();
    let mut levels = initial_levels;

    let player = ctx.player;
    let player_pos = player.pos.block_pos();
    let mut player_y = player_pos.y;

    for (y, _) in (player_y..=PLOT_BLOCK_HEIGHT).enumerate() {
        if levels == 0 {
            break;
        }
        let y = y as i32 + 1;

        let floor_pos = player_pos + BlockPos::new(0, y - 1, 0);
        let pos = player_pos + BlockPos::new(0, y, 0);
        let high_pos = player_pos + BlockPos::new(0, y + 1, 0);
        if ctx.plot.get_block(floor_pos) != Block::Air
            && ctx.plot.get_block(pos) == Block::Air
            && ctx.plot.get_block(high_pos) == Block::Air
        {
            player_y = pos.y;
            levels -= 1;
        }
    }

    if player_y == player_pos.y {
        player.send_error_message("No free spot above you found.");
    } else {
        let mut pos = player.pos;
        pos.y = player_y as f64;
        player.teleport(pos);
        player.send_worldedit_message(&format!("Ascended {} levels.", initial_levels - levels));
    }
}

pub(super) fn execute_descend(ctx: CommandExecuteContext<'_>) {
    let initial_levels = ctx.arguments[0].unwrap_uint();
    let mut levels = initial_levels;

    let player = ctx.player;
    let player_pos = player.pos.block_pos();
    let mut player_y = player_pos.y;

    for (y, _) in (1..player_y).enumerate() {
        if levels == 0 {
            break;
        }
        let y = -(y as i32 + 1);

        let floor_pos = player_pos + BlockPos::new(0, y - 1, 0);
        let pos = player_pos + BlockPos::new(0, y, 0);
        let high_pos = player_pos + BlockPos::new(0, y + 1, 0);
        if ctx.plot.get_block(floor_pos) != Block::Air
            && ctx.plot.get_block(pos) == Block::Air
            && ctx.plot.get_block(high_pos) == Block::Air
        {
            player_y = pos.y;
            levels -= 1;
        }
    }

    if player_y == player_pos.y {
        player.send_error_message("No free spot below you found.");
    } else {
        let mut pos = player.pos;
        pos.y = player_y as f64;
        player.teleport(pos);
        player.send_worldedit_message(&format!("Descended {} levels.", initial_levels - levels));
    }
}

pub(super) fn execute_rstack(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let stack_amt = ctx.arguments[0].unwrap_uint();
    let stack_spacing = ctx.arguments[1].unwrap_uint();
    let direction = ctx.arguments[2].unwrap_direction_vec();
    let pos1 = ctx.player.first_position.unwrap();
    let pos2 = ctx.player.second_position.unwrap();
    let clipboard = create_clipboard(ctx.plot, pos1, pos1, pos2);
    let mut undo_cbs = Vec::new();
    for i in 1..stack_amt + 1 {
        let offset = (i * stack_spacing) as i32;

        let block_pos = pos1 + direction * offset;
        undo_cbs.push(create_clipboard(
            ctx.plot,
            pos1,
            block_pos,
            pos2 + direction * offset,
        ));
        paste_clipboard(ctx.plot, &clipboard, block_pos, !ctx.has_flag('a'));
    }
    undo_cbs.reverse();
    let undo = WorldEditUndo {
        clipboards: undo_cbs,
        pos: pos1,
        plot_x: ctx.plot.x,
        plot_z: ctx.plot.z,
    };

    if ctx.has_flag('e') {
        expand_selection(
            ctx.player,
            direction * (stack_amt * stack_spacing) as i32,
            false,
        );
    }

    let player = ctx.player;
    player.worldedit_undo.push(undo);

    player.send_worldedit_message(&format!(
        "Your selection was stacked successfully. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_update(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let (first_pos, second_pos) = if ctx.has_flag('p') {
        ctx.plot.get_corners()
    } else if let (Some(first_pos), Some(second_pos)) =
        (ctx.player.first_position, ctx.player.second_position)
    {
        (first_pos, second_pos)
    } else {
        ctx.player
            .send_error_message("Your selection is incomplete.");
        return;
    };

    update(ctx.plot, first_pos, second_pos);

    ctx.player.send_worldedit_message(&format!(
        "Your selection was updated sucessfully. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_replace_container(ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let from = ctx.arguments[0].unwrap_container_type();
    let to = ctx.arguments[1].unwrap_container_type();

    let new_block = match to {
        ContainerType::Furnace => Block::Furnace {
            facing: BlockDirection::North,
            lit: false,
        },
        ContainerType::Barrel => Block::Barrel {
            open: false,
            facing: BlockFacing::Up,
        },
        ContainerType::Hopper => Block::Hopper {
            enabled: false,
            facing: HopperFacing::Down,
        },
    };
    let slots = to.num_slots() as u32;

    let operation = worldedit_start_operation(ctx.player);
    for x in operation.x_range() {
        for y in operation.y_range() {
            for z in operation.z_range() {
                let pos = BlockPos::new(x, y, z);
                let block = ctx.plot.get_block(pos);

                if !matches!(
                    block,
                    Block::Furnace { .. } | Block::Barrel { .. } | Block::Hopper { .. }
                ) {
                    continue;
                }
                let block_entity = ctx.plot.get_block_entity(pos);
                if let Some(BlockEntity::Container {
                    comparator_override,
                    ty,
                    ..
                }) = block_entity
                {
                    if *ty != from {
                        continue;
                    }
                    let ss = *comparator_override;

                    let items_needed = match ss {
                        0 => 0,
                        15 => slots * 64,
                        _ => ((32 * slots * ss as u32) as f32 / 7.0 - 1.0).ceil() as u32,
                    } as usize;
                    let mut inventory = Vec::new();
                    for (slot, items_added) in (0..items_needed).step_by(64).enumerate() {
                        let count = (items_needed - items_added).min(64);
                        inventory.push(InventoryEntry {
                            id: Item::Redstone {}.get_id(),
                            slot: slot as i8,
                            count: count as i8,
                            nbt: None,
                        });
                    }

                    let new_entity = BlockEntity::Container {
                        comparator_override: ss,
                        inventory,
                        ty: to,
                    };
                    ctx.plot.set_block_entity(pos, new_entity);
                    ctx.plot.set_block(pos, new_block);
                }
            }
        }
    }

    ctx.player.send_worldedit_message(&format!(
        "Your selection was replaced sucessfully. ({:?})",
        start_time.elapsed()
    ));
}

pub(super) fn execute_unimplemented(_ctx: CommandExecuteContext<'_>) {
    unimplemented!("Unimplimented worldedit command");
}

// -----------------------------------------------------------------------------
// ROM system commands and helpers
// -----------------------------------------------------------------------------
//
// These commands paste a ROM-based lookup table from binary weights and then add
// the addressing, read-line, and hex-to-bin converter circuits around it.
//
// Coordinate convention used by the generated system:
// - X grows to the right side of the ROM grid.
// - Z depth grows toward negative Z.
// - Y grows upward.
// - One ROM tile stores four addresses.
// - One ROM tile covers two ROM cells in depth.
// - One vertical ROM layer stores four bits of one weight.

const ROM_CELL_BYTES: &[u8] = include_bytes!("../../../assets/rom_4x4_cell.schem");
const ROM_CELL_SINGLE_BYTES: &[u8] =
    include_bytes!("../../../assets/rom_4x4_cell_full_orange.schem");

const ADDRESSING_PART_1_YELLOW_BYTES: &[u8] =
    include_bytes!("../../../assets/addressing_part_1_yellow.schem");
const ADDRESSING_PART_2_YELLOW_BYTES: &[u8] =
    include_bytes!("../../../assets/addressing_part_2_yellow.schem");
const READLINE_RED_BYTES: &[u8] = include_bytes!("../../../assets/readline_red_v2.schem");
const HEX_2_BIN_LIME_BYTES: &[u8] = include_bytes!("../../../assets/hex-2-bin_lime.schem");

fn read_bits_lsb_first(bytes: &[u8], start_bit: usize, bit_count: usize) -> u32 {
    let mut value: u32 = 0;

    for bit_offset in 0..bit_count {
        let bit_index = start_bit + bit_offset;
        let byte_index = bit_index / 8;
        let bit_in_byte = bit_index % 8;

        let bit = (bytes[byte_index] >> bit_in_byte) & 1;
        value |= (bit as u32) << bit_offset;
    }

    value
}

fn read_rom_weight_file(
    path: &std::path::Path,
    weight_bits: usize,
) -> Result<Vec<Vec<u8>>, String> {
    if weight_bits == 0 {
        return Err("weight_bits must be greater than 0.".to_string());
    }

    if weight_bits > 32 {
        return Err("weight_bits greater than 32 is not supported yet.".to_string());
    }

    let bytes =
        std::fs::read(path).map_err(|e| format!("Could not read binary weight file: {e}"))?;

    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    let total_bits = bytes.len() * 8;

    if total_bits % weight_bits != 0 {
        return Err(format!(
            "Binary file does not contain a whole number of {}-bit weights. Total bits: {}.",
            weight_bits, total_bits
        ));
    }

    let weight_count = total_bits / weight_bits;

    // Each barrel/comparator layer can represent one 4-bit signal, from 0 to 15.
    // Therefore an 8-bit weight needs 2 vertical layers, an 18-bit weight needs
    // 5 vertical layers, and so on. The final layer may contain fewer than 4
    // meaningful bits when weight_bits is not divisible by 4.
    let vertical_stacks = weight_bits.div_ceil(4);

    let mut weights: Vec<Vec<u8>> = Vec::with_capacity(weight_count);

    for weight_index in 0..weight_count {
        let start_bit = weight_index * weight_bits;
        let raw_weight = read_bits_lsb_first(&bytes, start_bit, weight_bits);

        let mut barrel_values: Vec<u8> = Vec::with_capacity(vertical_stacks);

        for layer in 0..vertical_stacks {
            let signal = ((raw_weight >> (layer * 4)) & 0xF) as u8;
            barrel_values.push(signal);
        }

        weights.push(barrel_values);
    }

    Ok(weights)
}

fn inventory_for_signal(signal: u8) -> Vec<InventoryEntry> {
    assert!(signal <= 15);

    let slots = 27u32;

    // Minecraft comparator output is determined by container fullness.
    // This converts the target signal strength into the amount of redstone dust
    // required in a 27-slot barrel.
    let items_needed = match signal {
        0 => 0,
        15 => slots * 64,
        _ => ((32 * slots * signal as u32) as f32 / 7.0 - 1.0).ceil() as u32,
    } as usize;

    let mut inventory = Vec::new();

    for (slot, items_added) in (0..items_needed).step_by(64).enumerate() {
        let count = (items_needed - items_added).min(64);

        inventory.push(InventoryEntry {
            id: Item::Redstone {}.get_id(),
            slot: slot as i8,
            count: count as i8,
            nbt: None,
        });
    }

    inventory
}

fn romtile_barrel_offset(local_address: usize) -> BlockPos {
    match local_address {
        // Four local addresses are stored inside one ROM tile schematic.
        // These offsets point from the tile origin to the barrel that belongs
        // to each address.
        0 => BlockPos::new(-1, 3, -4),
        1 => BlockPos::new(-1, 2, -6),
        2 => BlockPos::new(1, 2, -6),
        3 => BlockPos::new(1, 3, -4),
        _ => unreachable!("local_address must be between 0 and 3"),
    }
}

struct RomTilePlan {
    weights: Vec<Vec<u8>>,
    address_count: usize,
    vertical_stacks: usize,
    x_count: usize,
    z_count: usize,
}

fn build_romtile_plan(
    ctx: &mut CommandExecuteContext<'_>,
    weight_bits: usize,
    build_depth: usize,
    file_name: &str,
) -> Option<RomTilePlan> {
    if weight_bits == 0 {
        ctx.player
            .send_error_message("weight_bits must be greater than 0.");
        return None;
    }

    if weight_bits > 32 {
        ctx.player
            .send_error_message("weight_bits greater than 32 is not supported yet.");
        return None;
    }

    if build_depth == 0 {
        ctx.player
            .send_error_message("build_depth must be greater than 0.");
        return None;
    }

    if build_depth % 4 != 0 {
        ctx.player
            .send_error_message("build_depth must be a multiple of 4.");
        return None;
    }

    // Keep weight files inside ./weights and prevent path traversal.
    if file_name.contains("..") || file_name.contains('/') || file_name.contains('\\') {
        ctx.player.send_error_message("Invalid weight filename.");
        return None;
    }

    let vertical_stacks = weight_bits.div_ceil(4);

    // build_depth is measured in ROM cells.
    // One ROM tile schematic contains two cells in the negative-Z direction.
    let z_count = build_depth / 2;

    let path = PathBuf::from("./weights").join(file_name);

    let weights = match read_rom_weight_file(&path, weight_bits) {
        Ok(w) => w,
        Err(e) => {
            ctx.player
                .send_error_message(&format!("Failed to read ROM weight file: {e}"));
            return None;
        }
    };

    let address_count = weights.len();

    if address_count == 0 {
        ctx.player.send_error_message("Weight file is empty.");
        return None;
    }

    if address_count % 4 != 0 {
        ctx.player.send_error_message(&format!(
            "Weight count must be a multiple of 4 because one ROM tile stores 4 addresses. Got {}.",
            address_count
        ));
        return None;
    }

    let romtile_count = address_count / 4;

    if romtile_count % z_count != 0 {
        ctx.player.send_error_message(&format!(
            "The binary file does not fit the requested build_depth. ROM tile count ({}) must be divisible by z_count ({}). Try another build_depth.",
            romtile_count,
            z_count
        ));
        return None;
    }

    let x_count = romtile_count / z_count;

    Some(RomTilePlan {
        weights,
        address_count,
        vertical_stacks,
        x_count,
        z_count,
    })
}

fn paste_romtile_plan_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    origin: BlockPos,
    plan: &RomTilePlan,
) -> Option<usize> {
    let cell_multi_color = match load_schematic_from_reader(std::io::Cursor::new(ROM_CELL_BYTES)) {
        Ok(c) => c,
        Err(e) => {
            ctx.player
                .send_error_message(&format!("Failed to load ROM cell schematic: {e}"));
            return None;
        }
    };

    let cell_single_color =
        match load_schematic_from_reader(std::io::Cursor::new(ROM_CELL_SINGLE_BYTES)) {
            Ok(c) => c,
            Err(e) => {
                ctx.player
                    .send_error_message(&format!("Failed to load ROM cell schematic: {e}"));
                return None;
            }
        };

    let mut cell = if ctx.has_flag('s') {
        cell_single_color
    } else {
        cell_multi_color
    };

    // Align the schematic's internal origin with the logical ROM tile origin.
    // This is the same placement offset used by the original standalone ROM tile
    // command, so the system command keeps exactly the same geometry.
    cell.offset_x = 2;
    cell.offset_y = -1;
    cell.offset_z = 6;

    let undo_end = BlockPos::new(
        origin.x + (plan.x_count as i32 - 1) * 4 + cell.size_x as i32 - 1,
        origin.y + (plan.vertical_stacks as i32 - 1) * 2 + cell.size_y as i32 - 1,
        origin.z - (plan.z_count as i32 - 1) * 4 + cell.size_z as i32 - 1,
    );

    capture_undo(ctx.plot, ctx.player, origin, undo_end);

    let positions: Vec<BlockPos> = (0..plan.vertical_stacks)
        .flat_map(|layer| {
            (0..plan.x_count).flat_map(move |xi| {
                (0..plan.z_count).map(move |zi| {
                    BlockPos::new(
                        origin.x + xi as i32 * 4,
                        origin.y + layer as i32 * 2,
                        origin.z - zi as i32 * 4,
                    )
                })
            })
        })
        .collect();

    for chunk in positions.chunks(5000) {
        paste_clipboard_batch(ctx.plot, &cell, chunk, true);
    }

    let mut barrels_written = 0usize;

    for (address, barrel_values) in plan.weights.iter().enumerate() {
        //let romtile_index = address / 4;
        let xi = address / (plan.z_count * 4); //romtile_index / plan.z_count;
        let zi = (address % (plan.z_count * 2)) / 2;

        let local_address = match (xi % 2, (address % (plan.z_count * 2)) % 2, (address / (plan.z_count * 2)) % 2) {
            (0, 0, 0) => 0,
            (0, 1, 0) => 1,
            (0, 0, 1) => 3,
            (0, 1, 1) => 2,
            (1, 0, 0) => 3,
            (1, 1, 0) => 2,
            (1, 0, 1) => 0,
            (1, 1, 1) => 1,
            _ => unreachable!(),
        };

        

        for layer in 0..plan.vertical_stacks {
            let raw_signal = barrel_values[layer];

            // The physical redstone ROM uses inverted comparator strength.
            // A raw 0 therefore becomes 15, and a raw 15 becomes 0.
            let signal = 15u8 - raw_signal;

            let tile_origin = BlockPos::new(
                origin.x + xi as i32 * 4,
                origin.y + layer as i32 * 2,
                origin.z - zi as i32 * 4,
            );

            let barrel_pos = tile_origin + romtile_barrel_offset(local_address);
            let inventory = inventory_for_signal(signal);

            let new_entity = BlockEntity::Container {
                comparator_override: signal,
                inventory,
                ty: ContainerType::Barrel,
            };

            ctx.plot.set_block(
                barrel_pos,
                Block::Barrel {
                    open: false,
                    facing: BlockFacing::Up,
                },
            );

            ctx.plot.set_block_entity(barrel_pos, new_entity);

            barrels_written += 1;
        }
    }

    Some(barrels_written)
}

fn paste_tiled_addressing_part_1_yellow_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    width: usize,
    depth: usize,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    if width == 0 || depth == 0 {
        ctx.player
            .send_error_message("width and depth must be greater than 0.");
        return false;
    }

    let mut schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    // Same embedded-schematic correction used by the manually placed modules.
    // The command receives a logical origin, then the schematic offset is shifted
    // so the physical structure lands at the expected relative position.
    schematic.offset_x += 2;
    schematic.offset_y -= 4;
    schematic.offset_z += 1;

    // addr1y is the bottom addressing base of the ROM system.
    // It follows the same ROM grid spacing: +4 on X and -4 on Z.
    let step_x = 4;
    let step_z = 4;

    let mut undo_min_x = i32::MAX;
    let mut undo_min_y = i32::MAX;
    let mut undo_min_z = i32::MAX;

    let mut undo_max_x = i32::MIN;
    let mut undo_max_y = i32::MIN;
    let mut undo_max_z = i32::MIN;

    for x_index in 0..width {
        for z_index in 0..depth {
            let paste_origin = BlockPos::new(
                origin.x + x_index as i32 * step_x,
                origin.y,
                origin.z - z_index as i32 * step_z,
            );

            let first_pos = BlockPos::new(
                paste_origin.x - schematic.offset_x,
                paste_origin.y - schematic.offset_y,
                paste_origin.z - schematic.offset_z,
            );

            let second_pos = BlockPos::new(
                first_pos.x + schematic.size_x as i32 - 1,
                first_pos.y + schematic.size_y as i32 - 1,
                first_pos.z + schematic.size_z as i32 - 1,
            );

            undo_min_x = undo_min_x.min(first_pos.x).min(second_pos.x);
            undo_min_y = undo_min_y.min(first_pos.y).min(second_pos.y);
            undo_min_z = undo_min_z.min(first_pos.z).min(second_pos.z);

            undo_max_x = undo_max_x.max(first_pos.x).max(second_pos.x);
            undo_max_y = undo_max_y.max(first_pos.y).max(second_pos.y);
            undo_max_z = undo_max_z.max(first_pos.z).max(second_pos.z);
        }
    }

    let undo_first = BlockPos::new(undo_min_x, undo_min_y, undo_min_z);
    let undo_second = BlockPos::new(undo_max_x, undo_max_y, undo_max_z);

    capture_undo(ctx.plot, ctx.player, undo_first, undo_second);

    for x_index in 0..width {
        for z_index in 0..depth {
            let paste_origin = BlockPos::new(
                origin.x + x_index as i32 * step_x,
                origin.y,
                origin.z - z_index as i32 * step_z,
            );

            // Skip air blocks so overlapping tiles do not erase each other.
            paste_clipboard(ctx.plot, &schematic, paste_origin, true);
        }
    }

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} as a {} x {} grid. ({:?})",
            schematic_name,
            width,
            depth,
            start_time.elapsed()
        ));
    }

    true
}

fn paste_tiled_addressing_part_2_yellow_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    width: usize,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    if width == 0 {
        ctx.player
            .send_error_message("width must be greater than 0.");
        return false;
    }

    let mut schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    // Same embedded-schematic correction used by addr1y.
    schematic.offset_x += 2;
    schematic.offset_y -= 4;
    schematic.offset_z += 1;

    // addr2y forms the second addressing strip on the corner edge of addr1y.
    // It is repeated only across the ROM width and uses the same 4-block X grid.
    let step_x = 8;

    let mut undo_min_x = i32::MAX;
    let mut undo_min_y = i32::MAX;
    let mut undo_min_z = i32::MAX;

    let mut undo_max_x = i32::MIN;
    let mut undo_max_y = i32::MIN;
    let mut undo_max_z = i32::MIN;

    for x_index in 0..width {
        let paste_origin = BlockPos::new(origin.x + x_index as i32 * step_x, origin.y, origin.z);

        let first_pos = BlockPos::new(
            paste_origin.x - schematic.offset_x,
            paste_origin.y - schematic.offset_y,
            paste_origin.z - schematic.offset_z,
        );

        let second_pos = BlockPos::new(
            first_pos.x + schematic.size_x as i32 - 1,
            first_pos.y + schematic.size_y as i32 - 1,
            first_pos.z + schematic.size_z as i32 - 1,
        );

        undo_min_x = undo_min_x.min(first_pos.x).min(second_pos.x);
        undo_min_y = undo_min_y.min(first_pos.y).min(second_pos.y);
        undo_min_z = undo_min_z.min(first_pos.z).min(second_pos.z);

        undo_max_x = undo_max_x.max(first_pos.x).max(second_pos.x);
        undo_max_y = undo_max_y.max(first_pos.y).max(second_pos.y);
        undo_max_z = undo_max_z.max(first_pos.z).max(second_pos.z);
    }

    let undo_first = BlockPos::new(undo_min_x, undo_min_y, undo_min_z);
    let undo_second = BlockPos::new(undo_max_x, undo_max_y, undo_max_z);

    capture_undo(ctx.plot, ctx.player, undo_first, undo_second);

    for x_index in 0..width {
        let paste_origin = BlockPos::new(origin.x + x_index as i32 * step_x, origin.y, origin.z);

        // Skip air blocks so overlapping strips do not erase each other.
        paste_clipboard(ctx.plot, &schematic, paste_origin, true);
    }

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} {} time(s) along X with a 4-block origin step. ({:?})",
            schematic_name,
            width,
            start_time.elapsed()
        ));
    }

    true
}

fn paste_tiled_readline_red_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    width: usize,
    length: usize,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    if width == 0 || length == 0 {
        ctx.player
            .send_error_message("width and length must be greater than 0.");
        return false;
    }

    let mut schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    // The readline schematic uses the same logical-origin correction as the
    // yellow addressing schematics.
    schematic.offset_x += 2;
    schematic.offset_y -= 4;
    schematic.offset_z += 1;

    // readline_red is copied across ROM width and upward for each vertical ROM
    // layer. This matches the ROM tile grid: +4 on X and +2 on Y.
    let step_x = 4;
    let step_y = 2;

    let mut undo_min_x = i32::MAX;
    let mut undo_min_y = i32::MAX;
    let mut undo_min_z = i32::MAX;

    let mut undo_max_x = i32::MIN;
    let mut undo_max_y = i32::MIN;
    let mut undo_max_z = i32::MIN;

    for x_index in 0..width {
        for y_index in 0..length {
            let paste_origin = BlockPos::new(
                origin.x + x_index as i32 * step_x,
                origin.y + y_index as i32 * step_y,
                origin.z,
            );

            let first_pos = BlockPos::new(
                paste_origin.x - schematic.offset_x,
                paste_origin.y - schematic.offset_y,
                paste_origin.z - schematic.offset_z,
            );

            let second_pos = BlockPos::new(
                first_pos.x + schematic.size_x as i32 - 1,
                first_pos.y + schematic.size_y as i32 - 1,
                first_pos.z + schematic.size_z as i32 - 1,
            );

            undo_min_x = undo_min_x.min(first_pos.x).min(second_pos.x);
            undo_min_y = undo_min_y.min(first_pos.y).min(second_pos.y);
            undo_min_z = undo_min_z.min(first_pos.z).min(second_pos.z);

            undo_max_x = undo_max_x.max(first_pos.x).max(second_pos.x);
            undo_max_y = undo_max_y.max(first_pos.y).max(second_pos.y);
            undo_max_z = undo_max_z.max(first_pos.z).max(second_pos.z);
        }
    }

    let undo_first = BlockPos::new(undo_min_x, undo_min_y, undo_min_z);
    let undo_second = BlockPos::new(undo_max_x, undo_max_y, undo_max_z);

    capture_undo(ctx.plot, ctx.player, undo_first, undo_second);

    for x_index in 0..width {
        for y_index in 0..length {
            let paste_origin = BlockPos::new(
                origin.x + x_index as i32 * step_x,
                origin.y + y_index as i32 * step_y,
                origin.z,
            );

            // Skip air blocks so overlapping read-line pieces do not erase
            // neighboring redstone circuits.
            paste_clipboard(ctx.plot, &schematic, paste_origin, true);
        }
    }

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} as a {} x {} grid with X step 4 and Y step 2. ({:?})",
            schematic_name,
            width,
            length,
            start_time.elapsed()
        ));
    }

    true
}

fn paste_stacked_hex_2_bin_lime_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    layers: usize,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    if layers == 0 {
        ctx.player
            .send_error_message("layers must be greater than 0.");
        return false;
    }

    let schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    // In the full ROM system, this schematic is already authored with its
    // origin at the top north-east corner, so no addr/readline offset correction
    // is applied. Each extra converter is stacked downward by two blocks.
    let step_y = -2;

    let mut undo_min_x = i32::MAX;
    let mut undo_min_y = i32::MAX;
    let mut undo_min_z = i32::MAX;

    let mut undo_max_x = i32::MIN;
    let mut undo_max_y = i32::MIN;
    let mut undo_max_z = i32::MIN;

    for layer in 0..layers {
        let paste_origin = BlockPos::new(origin.x, origin.y + layer as i32 * step_y, origin.z);

        let first_pos = BlockPos::new(
            paste_origin.x - schematic.offset_x,
            paste_origin.y - schematic.offset_y,
            paste_origin.z - schematic.offset_z,
        );

        let second_pos = BlockPos::new(
            first_pos.x + schematic.size_x as i32 - 1,
            first_pos.y + schematic.size_y as i32 - 1,
            first_pos.z + schematic.size_z as i32 - 1,
        );

        undo_min_x = undo_min_x.min(first_pos.x).min(second_pos.x);
        undo_min_y = undo_min_y.min(first_pos.y).min(second_pos.y);
        undo_min_z = undo_min_z.min(first_pos.z).min(second_pos.z);

        undo_max_x = undo_max_x.max(first_pos.x).max(second_pos.x);
        undo_max_y = undo_max_y.max(first_pos.y).max(second_pos.y);
        undo_max_z = undo_max_z.max(first_pos.z).max(second_pos.z);
    }

    let undo_first = BlockPos::new(undo_min_x, undo_min_y, undo_min_z);
    let undo_second = BlockPos::new(undo_max_x, undo_max_y, undo_max_z);

    capture_undo(ctx.plot, ctx.player, undo_first, undo_second);

    for layer in 0..layers {
        let paste_origin = BlockPos::new(origin.x, origin.y + layer as i32 * step_y, origin.z);

        // Skip air blocks so the converter does not erase existing circuits.
        paste_clipboard(ctx.plot, &schematic, paste_origin, true);
    }

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} as {} layer(s) stacked in -Y with 2-block spacing. ({:?})",
            schematic_name,
            layers,
            start_time.elapsed()
        ));
    }

    true
}

pub(super) fn execute_rom_system(mut ctx: CommandExecuteContext<'_>) {
    let start_time = Instant::now();

    let weight_bits = ctx.arguments[0].unwrap_uint() as usize;
    let build_depth = ctx.arguments[1].unwrap_uint() as usize;
    let file_name = ctx.arguments[2].unwrap_string().clone();

    let Some(selected_origin) = ctx.player.first_position else {
        ctx.player.send_error_message("You must set //pos1 first.");
        return;
    };

    // The player selects the desired addr3y start position with //pos1.
    // addr3y is placed at addr_origin +1 X, -6 Y, +18 Z,
    // so the real ROM system origin must be shifted in the opposite direction.
    let addr_origin = BlockPos::new(
        selected_origin.x - 2,
        selected_origin.y + 6,
        selected_origin.z - 18,
    );

    let Some(plan) = build_romtile_plan(&mut ctx, weight_bits, build_depth, &file_name) else {
        return;
    };

    // The ROM plan determines the size of every schematic group:
    // - width: number of ROM tiles on the X axis
    // - depth: number of ROM tiles on the negative-Z axis
    // - height: number of 4-bit layers needed for each weight
    let system_width = plan.x_count;
    let system_depth = plan.z_count;
    let system_height = plan.vertical_stacks;

    let addressing_part_1_ok = paste_tiled_addressing_part_1_yellow_at_origin(
        &mut ctx,
        ADDRESSING_PART_1_YELLOW_BYTES,
        "addressing_part_1_yellow.schem",
        system_width,
        system_depth,
        addr_origin,
        false,
    );

    if !addressing_part_1_ok {
        return;
    }

    // addr2y starts four blocks in +Z from addr1y and only repeats across width.
    let addr2_origin = BlockPos::new(addr_origin.x, addr_origin.y - 3, addr_origin.z + 5);

    let addr2_count = system_width / 2 + 1;

    let addressing_part_2_ok = paste_tiled_addressing_part_2_yellow_at_origin(
        &mut ctx,
        ADDRESSING_PART_2_YELLOW_BYTES,
        "addressing_part_2_yellow.schem",
        addr2_count,
        addr2_origin,
        false,
    );

    if !addressing_part_2_ok {
        return;
    }

    // ROM tiles are positioned above and slightly forward from the addr1y base.
    let rom_origin = BlockPos::new(addr_origin.x + 2, addr_origin.y + 4, addr_origin.z + 1);

    let Some(barrels_written) = paste_romtile_plan_at_origin(&mut ctx, rom_origin, &plan) else {
        return;
    };

    // readline_red follows the ROM width and vertical ROM height.
    let readline_origin = BlockPos::new(addr_origin.x + 4, addr_origin.y + 3, addr_origin.z + 3);

    let readline_ok = paste_tiled_readline_red_at_origin(
        &mut ctx,
        READLINE_RED_BYTES,
        "readline_red.schem",
        system_width,
        system_height,
        readline_origin,
        false,
    );

    if !readline_ok {
        return;
    }

    // Hex-to-bin lime converter is placed on the right side of the orange ROM
    // body. Its origin is the top north-east corner of the schematic, and each
    // additional copy is stacked downward in -Y by two blocks.
    let hex_2_bin_origin = BlockPos::new(
        rom_origin.x + (system_width as i32 - 1) * 4 + 1,
        rom_origin.y + (system_height as i32 - 1) * 2 + 3,
        rom_origin.z + 2,
    );

    let hex_2_bin_ok = paste_stacked_hex_2_bin_lime_at_origin(
        &mut ctx,
        HEX_2_BIN_LIME_BYTES,
        "hex-2-bin_lime.schem",
        system_height,
        hex_2_bin_origin,
        false,
    );

    if !hex_2_bin_ok {
        return;
    }

    // addr3y is placed in front of addr2y.
    // It starts from the ROM system origin with offset +1 X, -5 Y, +17 Z.
    // It extends along X together with addr2y, but only one addr3y is placed
    // for every 4 addr2y modules.
    let addr3_count = system_width / 4 + 1;

    let addr3_origin = BlockPos::new(addr_origin.x - 3, addr_origin.y - 5, addr_origin.z + 19);

    let addressing_part_3_ok = paste_repeated_addressing_part_3_yellow_at_origin(
        &mut ctx,
        ADDRESSING_PART_3_YELLOW_BYTES,
        "addressing_part_3_yellow.schem",
        addr3_count,
        addr3_origin,
        false,
    );

    if !addressing_part_3_ok {
        return;
    }

    // addr4y is placed once relative to the current ROM system origin.
    // Offset from addr_origin: -2 X, +9 Y, +1 Z.
    let addr4_origin = BlockPos::new(addr_origin.x - 5, addr_origin.y + 3, addr_origin.z + 19);

    let addressing_part_4_ok = paste_addressing_part_4_yellow_at_origin(
        &mut ctx,
        ADDRESSING_PART_4_YELLOW_BYTES,
        "addressing_part_4_yellow.schem",
        addr4_origin,
        false,
    );

    if !addressing_part_4_ok {
        return;
    }

    ctx.player.send_worldedit_message(&format!(
        "ROM system placed: addr1y {} x {}, addr2y {} wide at +4 Z, ROM at offset +2 X, +4 Y, +1 Z, readline_red {} x {} at +4 X, +2 Y, +3 Z, hex-2-bin_lime stacked {} layer(s) downward, {} address(es), {} bit weight, build_depth {}, {} vertical layer(s), {} barrel(s). ({:?})",
        system_width,
        system_depth,
        system_width,
        system_width,
        system_height,
        system_height,
        plan.address_count,
        weight_bits,
        build_depth,
        system_height,
        barrels_written,
        start_time.elapsed()
    ));
}

const ADDRESSING_PART_3_YELLOW_BYTES: &[u8] =
    include_bytes!("../../../assets/addressing_part_3_yellow.schem");

fn paste_repeated_addressing_part_3_yellow_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    count: usize,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    if count == 0 {
        return true;
    }

    let schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    // Do not apply any extra offset correction here.
    // addr3y must use its own saved schematic origin exactly.

    let step_x = 16; // 4 addr2y * 4 blocks per addr2y origin step

    let mut undo_min_x = i32::MAX;
    let mut undo_min_y = i32::MAX;
    let mut undo_min_z = i32::MAX;

    let mut undo_max_x = i32::MIN;
    let mut undo_max_y = i32::MIN;
    let mut undo_max_z = i32::MIN;

    for index in 0..count {
        let paste_origin = BlockPos::new(origin.x + index as i32 * step_x, origin.y, origin.z);

        let first_pos = BlockPos::new(
            paste_origin.x - schematic.offset_x,
            paste_origin.y - schematic.offset_y,
            paste_origin.z - schematic.offset_z,
        );

        let second_pos = BlockPos::new(
            first_pos.x + schematic.size_x as i32 - 1,
            first_pos.y + schematic.size_y as i32 - 1,
            first_pos.z + schematic.size_z as i32 - 1,
        );

        undo_min_x = undo_min_x.min(first_pos.x).min(second_pos.x);
        undo_min_y = undo_min_y.min(first_pos.y).min(second_pos.y);
        undo_min_z = undo_min_z.min(first_pos.z).min(second_pos.z);

        undo_max_x = undo_max_x.max(first_pos.x).max(second_pos.x);
        undo_max_y = undo_max_y.max(first_pos.y).max(second_pos.y);
        undo_max_z = undo_max_z.max(first_pos.z).max(second_pos.z);
    }

    let undo_first = BlockPos::new(undo_min_x, undo_min_y, undo_min_z);
    let undo_second = BlockPos::new(undo_max_x, undo_max_y, undo_max_z);

    capture_undo(ctx.plot, ctx.player, undo_first, undo_second);

    for index in 0..count {
        let paste_origin = BlockPos::new(origin.x + index as i32 * step_x, origin.y, origin.z);

        // Skip air blocks so addr3y does not erase addr2y or nearby wiring.
        paste_clipboard(ctx.plot, &schematic, paste_origin, true);
    }

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} {} time(s), one after every 4 addr2y modules, with X step 16. ({:?})",
            schematic_name,
            count,
            start_time.elapsed()
        ));
    }

    true
}

const ADDRESSING_PART_4_YELLOW_BYTES: &[u8] =
    include_bytes!("../../../assets/addressing_part_4_yellow.schem");

fn paste_addressing_part_4_yellow_at_origin(
    ctx: &mut CommandExecuteContext<'_>,
    schematic_bytes: &[u8],
    schematic_name: &str,
    origin: BlockPos,
    send_message: bool,
) -> bool {
    let start_time = Instant::now();

    let schematic = match load_schematic_from_reader(std::io::Cursor::new(schematic_bytes)) {
        Ok(schematic) => schematic,
        Err(err) => {
            ctx.player
                .send_error_message(&format!("Failed to load {}: {}", schematic_name, err));
            return false;
        }
    };

    let first_pos = BlockPos::new(
        origin.x - schematic.offset_x,
        origin.y - schematic.offset_y,
        origin.z - schematic.offset_z,
    );

    let second_pos = BlockPos::new(
        first_pos.x + schematic.size_x as i32 - 1,
        first_pos.y + schematic.size_y as i32 - 1,
        first_pos.z + schematic.size_z as i32 - 1,
    );

    capture_undo(ctx.plot, ctx.player, first_pos, second_pos);

    // Skip air blocks so the schematic does not erase existing redstone.
    paste_clipboard(ctx.plot, &schematic, origin, true);

    if send_message {
        ctx.player.send_worldedit_message(&format!(
            "Pasted {} at //pos1. ({:?})",
            schematic_name,
            start_time.elapsed()
        ));
    }

    true
}

// Reads a 784-bit (98-byte) binary file from ./images/ and stamps a 28×28 block grid into
// the world at a fixed origin position (overridable via command arguments).
//
// File format
// -----------
// The 784 bits are laid out column-major: the first 28 bits describe column 0 (x = origin.x),
// rows 0-27 from bottom (+y=0) to top (+y=54). Column 1 follows immediately, etc.
// Within each byte the most-significant bit (bit 7) is consumed first.
//
// Block mapping
// -------------
//   bit = 1  →  RedstoneBlock
//   bit = 0  →  LightGrayConcrete
//
// World layout
// ------------
// Each grid cell sits at (origin.x + col*2, origin.y + row*2, origin.z).
// The single-block gap between adjacent cells is explicitly set to Air so that any
// pre-existing blocks in the region are cleared. The full footprint is 55×55×1
// (formula: (28-1)*2 + 1 = 55 per axis).
//
// Undo/redo
// ---------
// The entire 55×55×1 region is snapshotted via capture_undo before any block is written,
// so //undo and //redo work as expected.
pub(super) fn execute_image_place(ctx: CommandExecuteContext<'_>) {
    // Allow only plain filenames (no path separators) to prevent directory traversal.
    static FILE_VALIDATE_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.]+$").unwrap());

    let file_name = ctx.arguments[0].unwrap_string().clone();
    if !FILE_VALIDATE_REGEX.is_match(&file_name) {
        ctx.player.send_error_message("Filename is invalid");
        return;
    }

    let origin = BlockPos::new(
        ctx.arguments[1].unwrap_uint() as i32,
        ctx.arguments[2].unwrap_uint() as i32,
        ctx.arguments[3].unwrap_uint() as i32,
    );

    let path = PathBuf::from("./images").join(&file_name);
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ctx.player
                .send_error_message("The specified file could not be found.");
            return;
        }
        Err(e) => {
            error!("Error reading image_place file: {}", e);
            ctx.player
                .send_error_message("Error reading file. Check console for details.");
            return;
        }
    };

    // 28×28 = 784 bits → ceil(784/8) = 98 bytes minimum.
    if data.len() < 98 {
        ctx.player.send_error_message(
            "File is too small (need exactly 784 bits / 98 bytes).",
        );
        return;
    }

    // Snapshot the full 55×55×1 footprint so the operation is undoable.
    // second_pos is inclusive, so +54 gives us 55 blocks (0..=54).
    let second_pos = BlockPos::new(origin.x + 54, origin.y + 54, origin.z);
    capture_undo(ctx.plot, ctx.player, origin, second_pos);

    // Pass 1: place the 28×28 grid blocks.
    // col advances along +x, row advances along +y, each spaced 2 blocks apart.
    for col in 0i32..28 {
        for row in 0i32..28 {
            // Linear bit index in column-major order.
            let bit_idx = (col * 28 + row) as usize;
            let byte_idx = bit_idx / 8;
            // MSB of each byte is the earlier bit (bit_idx % 8 == 0 → shift 7).
            let bit_shift = 7 - (bit_idx % 8);
            let is_one = (data[byte_idx] >> bit_shift) & 1 == 1;

            let block = if is_one {
                Block::RedstoneBlock
            } else {
                Block::LightGrayConcrete
            };
            let pos = BlockPos::new(origin.x + col * 2, origin.y + row * 2, origin.z);
            ctx.plot.set_block_raw(pos, block.get_id());
        }
    }

    // Pass 2: fill every non-grid position in the 55×55 footprint with Air.
    // Grid cells occupy even (dx, dy) offsets; odd offsets are the gaps.
    let air_id = Block::Air.get_id();
    for dx in 0i32..55 {
        for dy in 0i32..55 {
            if dx % 2 == 0 && dy % 2 == 0 {
                continue; // grid cell — already written in pass 1
            }
            ctx.plot
                .set_block_raw(BlockPos::new(origin.x + dx, origin.y + dy, origin.z), air_id);
        }
    }

    ctx.player
        .send_worldedit_message("Image placed successfully (784 blocks).");
}
