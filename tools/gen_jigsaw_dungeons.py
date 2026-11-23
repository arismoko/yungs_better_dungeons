"""Writes the zombie, skeleton and small Nether dungeons as data, from YUNG's Better Dungeons' own data folder.

usage: python3 tools/gen_jigsaw_dungeons.py <YUNGs-Better-Dungeons>/Common/src/main/resources/data/betterdungeons

Both are `yungsapi:yung_jigsaw` structures, which the game assembles itself: their structures,
structure sets, template pools, templates, biome tags and loot tables are copied as they are.
Their processor lists name the original's Java processors; each is written here as processor
data the game has (`minecraft:rule`, `sunrise:pick`, `sunrise:yung_pillar`,
`sunrise:spawner_mobs`, `lithostitched:condition`).
"""
import json, os, shutil, sys

SRC = sys.argv[1]
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'data', 'betterdungeons')
DUNGEONS = ['zombie_dungeon', 'skeleton_dungeon', 'small_nether_dungeon']

AIR = ['minecraft:air', 'minecraft:cave_air', 'minecraft:void_air']
OPEN = AIR + ['minecraft:water', 'minecraft:lava']


def st(i, **p):
    return {'id': i, 'properties': p} if p else {'id': i}


def where(blocks, location=False):
    c = {'type': 'lithostitched:matching_blocks', 'blocks': blocks}
    if location:
        c['match_type'] = 'location'
    return c


def when(conditions, then, otherwise=None):
    p = {'processor_type': 'lithostitched:condition', 'if_true': conditions, 'then': then}
    if otherwise is not None:
        p['else'] = otherwise
    return p


def set_block(state):
    return {'processor_type': 'lithostitched:set_block',
            'state_provider': {'type': 'minecraft:simple_state_provider', 'state': state},
            'preserve_state': False}


DISCARD = {'processor_type': 'lithostitched:discard_input'}


def in_air(block, then, otherwise=None, blocks=AIR):
    """A template `block` whose world block is one of `blocks`."""
    return when([where([block]), where(blocks, location=True)], then, otherwise)


def pick(block, picks, default, keep=False):
    p = {'processor_type': 'sunrise:pick', 'block': block, 'picks': picks, 'default': default}
    if keep:
        p['keep_properties'] = True
    return p


def randomizer(default, entries):
    return {'entries': [{'blockState': st(b), 'probability': pr} for b, pr in entries], 'defaultBlockState': st(default)}


def cumulative(entries, outcome):
    total, out = 0.0, []
    for value, share in entries:
        total = round(total + share, 6)
        out.append(dict(below=total, **outcome(value)))
    return out


CANDLES = ['candle', 'white_candle', 'gray_candle', 'light_gray_candle', 'brown_candle', 'green_candle', 'purple_candle', 'black_candle']


def candles(lit):
    return [st('minecraft:' + c, candles=str(n), lit=str(lit).lower()) for c in CANDLES for n in range(1, 5)]


def spawner(mob, **fields):
    return dict({'processor_type': 'sunrise:spawner_mobs', 'mobs': [{'entity': {'id': mob}, 'weight': 1}],
                 'replacement': st('minecraft:spawner')}, **fields)


# CandleProcessor: a sea pickle becomes one to four candles of one of eight colours, lit one time
# in ten.
CANDLE = [pick('minecraft:sea_pickle', [{'below': 0.1, 'states': candles(True)}], {'states': candles(False)})]

# The stair selector of the zombie dungeon's stairs and main stairs (BlockStateRandomizer).
STAIR_ENTRIES = [('minecraft:mossy_cobblestone_stairs', 0.4), ('minecraft:cobblestone_slab', 0.1),
                 ('minecraft:mossy_cobblestone_slab', 0.1), ('minecraft:cave_air', 0.1),
                 ('minecraft:cobblestone', 0.1), ('minecraft:mossy_cobblestone', 0.1)]

SKULLS = ['skeleton_skull', 'wither_skeleton_skull', 'zombie_head', 'creeper_head', 'piglin_head',
          'player_head', 'dragon_head']
HEADS = [pick('minecraft:' + b, [{'below': 0.167, 'state': st('minecraft:' + b)}], {'state': st('minecraft:air')}, keep=True)
         for skull in SKULLS for b in (skull, skull.replace('_skull', '_wall_skull').replace('_head', '_wall_head'))]


def banner(block, patterns, name, colour):
    return {'state': st(block), 'nbt': {'id': 'minecraft:banner',
            'patterns': [{'pattern': 'minecraft:' + p, 'color': c} for p, c in patterns],
            'CustomName': json.dumps({'text': name, 'color': colour, 'italic': False})}}


NETHER_BANNERS = {
    'skeleton': banner('minecraft:black_wall_banner', [('curly_border', 'white'), ('stripe_center', 'white'), ('stripe_bottom', 'black'),
                       ('stripe_top', 'white'), ('creeper', 'white'), ('gradient', 'black')], 'Vengeful Banner', 'dark_gray'),
    'wither_skeleton': banner('minecraft:red_wall_banner', [('curly_border', 'black'), ('stripe_center', 'black'), ('stripe_bottom', 'red'),
                              ('creeper', 'black'), ('stripe_top', 'black'), ('gradient_up', 'black')], 'Banner of Decay', 'dark_purple'),
    'zombified_piglin': banner('minecraft:pink_wall_banner', [('stripe_left', 'green'), ('triangles_top', 'black'), ('triangles_top', 'pink'),
                               ('stripe_center', 'pink'), ('half_vertical_mirror', 'light_gray'), ('diagonal_right', 'green'),
                               ('creeper', 'white'), ('half_horizontal_mirror', 'pink'), ('rhombus', 'pink'), ('piglin', 'black'),
                               ('gradient_up', 'pink'), ('stripe_bottom', 'black')], 'Banner of Pork', 'light_purple'),
    'blaze': banner('minecraft:red_wall_banner', [('stripe_small', 'yellow'), ('triangle_top', 'red'), ('triangle_top', 'red'),
                    ('flower', 'orange'), ('skull', 'yellow'), ('curly_border', 'red'), ('gradient_up', 'black')], 'Banner of Rage', 'gold'),
}

NETHER_WITHER_GEAR = {
    'equipment': {slot: {'id': 'minecraft:netherite_' + piece, 'count': 1}
                  for slot, piece in (('feet', 'boots'), ('legs', 'leggings'), ('chest', 'chestplate'), ('head', 'helmet'))},
    'drop_chances': {'feet': 0.0, 'legs': 0.0, 'chest': 0.0, 'head': 0.0},
}
NETHER_WITHER_GEAR['equipment']['mainhand'] = {'id': 'minecraft:stone_sword', 'count': 1}

JAVA = {
    # HeadProcessor: a skull stays one time in six (0.167), else it is air. Nether blocks stay
    # as they are with the original's default config (`enableNetherBlocks`).
    'betterdungeons:head_processor': HEADS,
    'betterdungeons:small_nether_dungeon_head_processor': HEADS,
    'betterdungeons:nether_block_processor': [],
    'betterdungeons:candle_processor': CANDLE,
    # ZombieDungeonFlowerPotProcessor.
    'betterdungeons:zombie_dungeon_flower_pot_processor': [pick(
        'minecraft:potted_cornflower',
        cumulative([('minecraft:potted_cornflower', 0.1), ('minecraft:potted_brown_mushroom', 0.1),
                    ('minecraft:potted_red_mushroom', 0.1), ('minecraft:potted_dead_bush', 0.4),
                    ('minecraft:potted_poppy', 0.1)], lambda b: {'state': st(b)}),
        {'state': st('minecraft:flower_pot')})],
    # ZombieDungeonStairProcessor: in open air the stair is gone, else a worn stair or block.
    'betterdungeons:zombie_dungeon_stair_processor': [
        in_air('minecraft:cobblestone_stairs', [set_block(st('minecraft:cave_air'))]),
        pick('minecraft:cobblestone_stairs', cumulative(STAIR_ENTRIES, lambda b: {'state': st(b)}),
             {'state': st('minecraft:cobblestone_stairs')}, keep=True)],
    # ZombieDungeonCubbyProcessor: worn stairs and slabs; a slab under a top stair is a top slab.
    'betterdungeons:zombie_dungeon_cubby_processor': [
        when([where(['minecraft:cobblestone_stairs']),
              {'type': 'lithostitched:matching_blocks', 'blocks': ['minecraft:cobblestone_stairs'], 'properties': {'half': 'top'}}],
             [pick('minecraft:cobblestone_stairs',
                   cumulative([('minecraft:mossy_cobblestone_stairs', 0.4), ('minecraft:cobblestone_slab', 0.1),
                               ('minecraft:mossy_cobblestone_slab', 0.1)],
                              lambda b: {'state': st(b, type='top') if 'slab' in b else st(b)}),
                   {'state': st('minecraft:cobblestone_stairs')}, keep=True)],
             [pick('minecraft:cobblestone_stairs',
                   cumulative([('minecraft:mossy_cobblestone_stairs', 0.4), ('minecraft:cobblestone_slab', 0.1),
                               ('minecraft:mossy_cobblestone_slab', 0.1)], lambda b: {'state': st(b)}),
                   {'state': st('minecraft:cobblestone_stairs')}, keep=True)])],
    # ZombieMobSpawnerProcessor.
    'betterdungeons:zombie_mob_spawner_processor': [spawner('minecraft:zombie', max_nearby_entities=8)],
    # ZombieTombstoneSpawnerProcessor: black glass becomes a spawner of skeletons with iron swords.
    'betterdungeons:zombie_tombstone_spawner_processor': [pick('minecraft:black_stained_glass', [], {
        'state': st('minecraft:spawner'),
        'nbt': {'id': 'minecraft:mob_spawner', 'SpawnData': {'entity': {
            'id': 'minecraft:skeleton', 'equipment': {'mainhand': {'id': 'minecraft:iron_sword', 'count': 1}}}}}})],
    # ZombieRotProcessor: cobblestone, cyan terracotta and stairs in open air are gone.
    'betterdungeons:zombie_rot_processor': [
        in_air(b, [set_block(st('minecraft:cave_air'))])
        for b in ('minecraft:cobblestone', 'minecraft:cyan_terracotta', 'minecraft:cobblestone_stairs')],
    # ZombieMainStairsProcessor digs a staircase to the surface; it is not ported, and its marker
    # becomes air as the original does where the surface is out of reach.
    'betterdungeons:zombie_main_stairs_processor': [
        when([where(['minecraft:warped_stairs'])], [set_block(st('minecraft:cave_air'))])],
    # ZombieDungeonLegProcessor: a leg where the world is open, down to the ground; elsewhere the
    # world's block stays. Purpur slabs likewise become smooth stone slabs only in the open.
    'betterdungeons:zombie_dungeon_leg_processor': [
        in_air('minecraft:magenta_stained_glass',
               [{'processor_type': 'sunrise:yung_pillar', 'target_block': st('minecraft:magenta_stained_glass'),
                 'target_block_output': randomizer('minecraft:smooth_stone', []),
                 'pillar_states': randomizer('minecraft:cobblestone', [('minecraft:polished_andesite', 0.8)])}],
               [DISCARD], blocks=OPEN),
        in_air('minecraft:purpur_slab', [set_block(st('minecraft:smooth_stone_slab'))], [DISCARD], blocks=OPEN)],
    # SkeletonMobSpawnerProcessor.
    'betterdungeons:skeleton_mob_spawner_processor': [spawner(
        'minecraft:skeleton', required_player_range=18, max_nearby_entities=8, max_spawn_delay=650)],
    # RuinedStoneBrickProcessor: in open air gone, else mossy, cracked or plain bricks.
    'betterdungeons:skeleton_dungeon_ruined_stone_bricks_processor': [
        in_air('minecraft:yellow_stained_glass', [set_block(st('minecraft:cave_air'))]),
        pick('minecraft:yellow_stained_glass',
             cumulative([('minecraft:mossy_stone_bricks', 0.3), ('minecraft:cracked_stone_bricks', 0.2)], lambda b: {'state': st(b)}),
             {'state': st('minecraft:stone_bricks')}),
        in_air('minecraft:prismarine_brick_slab', [set_block(st('minecraft:cave_air'))]),
        pick('minecraft:prismarine_brick_slab', [{'below': 0.3, 'state': st('minecraft:mossy_stone_brick_slab', type='top')}],
             {'state': st('minecraft:stone_brick_slab', type='top')})],
    # SkeletonDungeonLegProcessor: cobblestone with a cobblestone column down to the ground.
    'betterdungeons:skeleton_dungeon_leg_processor': [
        {'processor_type': 'sunrise:yung_pillar', 'target_block': st('minecraft:blue_stained_glass'),
         'target_block_output': randomizer('minecraft:cobblestone', []),
         'pillar_states': randomizer('minecraft:cobblestone', [('minecraft:mossy_cobblestone', 0.5)])}],
    # SmallNetherDungeonLavaBlockProcessor: orange wool is lava.
    'betterdungeons:small_nether_dungeon_lava_block_processor': [pick('minecraft:orange_wool', [], {'state': st('minecraft:lava')})],
    # SmallNetherDungeonEntranceStairsProcessor: brick stairs become nether bricks. The nether
    # brick stair it also writes in front of them is not placed (a processor writes only its block).
    'betterdungeons:small_nether_dungeon_entrance_stairs_processor': [
        pick('minecraft:brick_stairs', [], {'state': st('minecraft:nether_bricks')})],
    # SmallNetherDungeonLegProcessor: nether brick legs down to the ground.
    'betterdungeons:small_nether_dungeon_leg_processor': [
        {'processor_type': 'sunrise:yung_pillar', 'target_block': st('minecraft:orange_terracotta'),
         'target_block_output': randomizer('minecraft:nether_bricks', []),
         'pillar_states': randomizer('minecraft:nether_bricks', [])}],
}

SPAWNER_MOBS = {'skeleton': {'id': 'minecraft:skeleton'}, 'zombified_piglin': {'id': 'minecraft:zombified_piglin'},
                'blaze': {'id': 'minecraft:blaze'}, 'wither_skeleton': dict({'id': 'minecraft:wither_skeleton'}, **NETHER_WITHER_GEAR)}


def parameterised(p):
    """The Java processors that take a parameter in their list."""
    kind = p['processor_type']
    if kind == 'betterdungeons:small_nether_dungeon_banner_processor':
        # Each placeholder wall banner is one of the dungeon's design one time in ten, else gone.
        # The original also stops at two banners a piece; processor data has no count.
        return [pick('minecraft:gray_wall_banner', [dict(below=0.1, **NETHER_BANNERS[p['dungeon_type']])],
                     {'state': st('minecraft:air')}, keep=True)]
    if kind == 'betterdungeons:small_nether_dungeon_mob_spawner_processor':
        mob = p['spawner_mob'].split(':')[1]
        return [dict(spawner('minecraft:' + mob), mobs=[{'entity': SPAWNER_MOBS[mob], 'weight': 1}])]
    return None


def state_json(s):
    """The 26.3 block state form: `id` and `properties`."""
    out = {'id': s['Name']}
    if s.get('Properties'):
        out['properties'] = s['Properties']
    return out


def translate(processors):
    out = []
    for p in processors:
        kind = p['processor_type']
        if kind == 'minecraft:rule':
            rules = []
            for r in p['rules']:
                r = dict(r)
                r['output_state'] = state_json(r['output_state'])
                rules.append(r)
            out.append({'processor_type': kind, 'rules': rules})
        else:
            out.extend(parameterised(p) or JAVA[kind])
    return out


def copy(sub):
    src = os.path.join(SRC, sub)
    dst = os.path.join(OUT, sub)
    if os.path.isdir(src):
        shutil.rmtree(dst, ignore_errors=True)
        shutil.copytree(src, dst)
    else:
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy(src, dst)


def nether_dungeon():
    """The small Nether dungeon is off in the original unless its config enables it
    (`smallNetherDungeons.enabled`, default false). Its structure is a plain YUNG jigsaw here, and its
    biome tag is left empty: a data pack that lists biomes in it turns the dungeon on."""
    path = os.path.join(OUT, 'worldgen/structure/small_nether_dungeon.json')
    structure = json.load(open(path))
    structure['type'] = 'yungsapi:yung_jigsaw'
    structure.setdefault('use_expansion_hack', False)
    structure.setdefault('adapt_noise', False)
    with open(path, 'w') as f:
        json.dump(structure, f, indent=2)
        f.write('\n')
    with open(os.path.join(OUT, 'tags/worldgen/biome/has_structure/small_nether_dungeon.json'), 'w') as f:
        json.dump({'replace': False, 'values': []}, f, indent=2)
        f.write('\n')


def main():
    for d in DUNGEONS:
        copy(f'structure/{d}')
        copy(f'worldgen/structure/{d}.json')
        copy(f'worldgen/structure_set/{d}s.json')
        copy(f'worldgen/template_pool/{d}')
        copy(f'worldgen/template_pool/{d}.json')
        copy(f'tags/worldgen/biome/has_structure/{d}.json')
        copy(f'loot_table/{d}')
        lists = os.path.join(SRC, 'worldgen/processor_list', d)
        out = os.path.join(OUT, 'worldgen/processor_list', d)
        os.makedirs(out, exist_ok=True)
        for name in sorted(os.listdir(lists)):
            data = json.load(open(os.path.join(lists, name)))
            with open(os.path.join(out, name), 'w') as f:
                json.dump({'processors': translate(data['processors'])}, f, indent=2)
                f.write('\n')
    nether_dungeon()


if __name__ == '__main__':
    main()
