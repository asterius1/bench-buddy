"""Minimal raw arcdps .zevtc reader: agents, skill names and every combat event. Standard library only."""
import struct
import zipfile
from pathlib import Path

# arcdps state changes used here
SC_ENTER_COMBAT, SC_EXIT_COMBAT = 1, 2
SC_SPAWN, SC_DESPAWN = 6, 7
SC_HEALTH = 8
SC_LOG_START, SC_LOG_END = 9, 10
SC_POV = 13
SC_POSITION, SC_VELOCITY, SC_FACING = 19, 20, 21
SC_ID_TO_GUID = 46
SC_EFFECT_GROUND, SC_EFFECT_AGENT = 60, 62
SC_ANIMATION_START, SC_ANIMATION_STOP = 67, 68


class Agent:
    __slots__ = ("addr", "prof", "elite", "name", "is_player", "species", "hitbox_w", "hitbox_h",
                 "instids", "first", "last")

    def __init__(self, addr, prof, elite, name, hw, hh):
        self.addr, self.prof, self.elite, self.name = addr, prof, elite, name
        self.is_player = elite != 0xFFFFFFFF
        self.species = None if self.is_player else prof & 0xFFFF
        self.hitbox_w, self.hitbox_h = hw, hh
        self.instids = set()
        self.first = self.last = None

    def __repr__(self):
        return f"<{self.name!r} species={self.species} addr={self.addr:x}>"


class Event:
    __slots__ = ("time", "src", "dst", "value", "buff_dmg", "overstack", "skill", "src_inst", "dst_inst",
                 "src_master", "dst_master", "iff", "buff", "result", "activation", "buffremove",
                 "ninety", "fifty", "moving", "sc", "flanking", "shields", "offcycle", "raw")


_EV = struct.Struct("<QQQiiIIHHHH12B4x")


def load(path):
    """Return (agents by addr, skills by id, list of Event)."""
    with zipfile.ZipFile(path) as z:
        d = z.read(z.namelist()[0])
    o = 16
    (n,) = struct.unpack_from("<I", d, o)
    o += 4
    agents = {}
    for _ in range(n):
        addr, prof, elite = struct.unpack_from("<QII", d, o)
        hw, _cond, hh = struct.unpack_from("<HhH", d, o + 22)
        name = d[o + 28:o + 92].split(b"\0")[0].decode("utf-8", "replace")
        agents[addr] = Agent(addr, prof, elite, name, hw, hh)
        o += 96
    (n,) = struct.unpack_from("<I", d, o)
    o += 4
    skills = {}
    for _ in range(n):
        (sid,) = struct.unpack_from("<i", d, o)
        skills[sid] = d[o + 4:o + 68].split(b"\0")[0].decode("utf-8", "replace")
        o += 68
    events = []
    for b in range(o, len(d) - 63, 64):
        f = _EV.unpack_from(d, b)
        e = Event()
        (e.time, e.src, e.dst, e.value, e.buff_dmg, e.overstack, e.skill, e.src_inst, e.dst_inst,
         e.src_master, e.dst_master, e.iff, e.buff, e.result, e.activation, e.buffremove,
         e.ninety, e.fifty, e.moving, e.sc, e.flanking, e.shields, e.offcycle) = f
        e.raw = d[b:b + 64]
        events.append(e)
        if e.sc == 0 or e.sc in (SC_ENTER_COMBAT, SC_EXIT_COMBAT, SC_SPAWN, SC_DESPAWN, SC_POSITION):
            a = agents.get(e.src)
            if a is not None:
                a.instids.add(e.src_inst)
                if a.first is None:
                    a.first = e.time
                a.last = e.time
    return agents, skills, events


def vec3(e):
    """x, y, z floats of a position/velocity event (stored over dst_agent and value)."""
    return struct.unpack_from("<fff", e.raw, 16)


def vec2(e):
    return struct.unpack_from("<ff", e.raw, 16)

