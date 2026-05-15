import { useDevices, useUsers } from '../api';

export type FilterState = {
  user: string;
  device: string;
  /** Local datetime as `YYYY-MM-DDTHH:mm` (matches `<input type="datetime-local">`). Empty = open-ended. */
  from: string;
  to: string;
  limit: number;
};

type Props = {
  value: FilterState;
  onChange: (next: FilterState) => void;
};

type PresetKey = 'today' | '24h' | '7d' | '30d' | 'all';

const PRESETS: { key: PresetKey; label: string }[] = [
  { key: 'today', label: 'Today' },
  { key: '24h', label: '24h' },
  { key: '7d', label: '7d' },
  { key: '30d', label: '30d' },
  { key: 'all', label: 'All' },
];

export default function Filters({ value, onChange }: Props) {
  const users = useUsers();
  const devices = useDevices(value.user || undefined);
  const set = <K extends keyof FilterState>(k: K, v: FilterState[K]) =>
    onChange({ ...value, [k]: v });

  const applyPreset = (kind: PresetKey) => {
    const { from, to } = presetRange(kind);
    onChange({ ...value, from, to });
  };

  return (
    <>
      <label>User</label>
      <select
        value={value.user}
        onChange={(e) => onChange({ ...value, user: e.target.value, device: '' })}
      >
        <option value="">— any —</option>
        {users.data?.users.map((u) => (
          <option key={u.user} value={u.user}>
            {u.user} ({u.location_count})
          </option>
        ))}
      </select>

      <label>Device</label>
      <select value={value.device} onChange={(e) => set('device', e.target.value)}>
        <option value="">— any —</option>
        {devices.data?.devices.map((d) => (
          <option key={`${d.user}/${d.device}`} value={d.device}>
            {d.device} ({d.location_count})
          </option>
        ))}
      </select>

      <label>Range</label>
      <div className="preset-row">
        {PRESETS.map((p) => (
          <button
            key={p.key}
            type="button"
            className={isPresetActive(p.key, value) ? 'preset active' : 'preset'}
            onClick={() => applyPreset(p.key)}
          >
            {p.label}
          </button>
        ))}
      </div>

      <label>From</label>
      <input
        type="datetime-local"
        value={value.from}
        onChange={(e) => set('from', e.target.value)}
      />

      <label>To</label>
      <input
        type="datetime-local"
        value={value.to}
        onChange={(e) => set('to', e.target.value)}
      />

      <label>Limit</label>
      <input
        type="number"
        min={1}
        max={100000}
        value={value.limit}
        onChange={(e) => set('limit', Number(e.target.value) || 1000)}
      />
    </>
  );
}

/** Convert a `<input type="datetime-local">` value (local time) to an RFC3339 string with timezone. */
export function localDatetimeToIso(local: string): string | undefined {
  if (!local) return undefined;
  const d = new Date(local);
  if (Number.isNaN(d.getTime())) return undefined;
  return d.toISOString();
}

function localDatetimeInputValue(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function presetRange(kind: PresetKey): { from: string; to: string } {
  if (kind === 'all') return { from: '', to: '' };
  const now = new Date();
  if (kind === 'today') {
    const start = new Date(now);
    start.setHours(0, 0, 0, 0);
    return { from: localDatetimeInputValue(start), to: localDatetimeInputValue(now) };
  }
  const ms: Record<Exclude<PresetKey, 'all' | 'today'>, number> = {
    '24h': 24 * 60 * 60 * 1000,
    '7d': 7 * 24 * 60 * 60 * 1000,
    '30d': 30 * 24 * 60 * 60 * 1000,
  };
  const from = new Date(now.getTime() - ms[kind]);
  return { from: localDatetimeInputValue(from), to: localDatetimeInputValue(now) };
}

function isPresetActive(kind: PresetKey, value: FilterState): boolean {
  if (kind === 'all') return !value.from && !value.to;
  if (!value.from || !value.to) return false;
  const target = presetRange(kind);
  // Tolerate sub-minute drift between when the preset was applied and "now".
  return value.from === target.from && minutesBetween(value.to, target.to) < 2;
}

function minutesBetween(a: string, b: string): number {
  const da = new Date(a).getTime();
  const db = new Date(b).getTime();
  if (Number.isNaN(da) || Number.isNaN(db)) return Infinity;
  return Math.abs(da - db) / 60000;
}
