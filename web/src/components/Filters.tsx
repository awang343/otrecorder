import { useDevices, useUsers } from '../api';

export type FilterState = {
  user: string;
  device: string;
  from: string;
  to: string;
  limit: number;
};

type Props = {
  value: FilterState;
  onChange: (next: FilterState) => void;
};

export default function Filters({ value, onChange }: Props) {
  const users = useUsers();
  const devices = useDevices(value.user || undefined);
  const set = <K extends keyof FilterState>(k: K, v: FilterState[K]) =>
    onChange({ ...value, [k]: v });

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

      <label>From (ISO or unix seconds)</label>
      <input
        type="text"
        placeholder="2025-01-01T00:00:00Z"
        value={value.from}
        onChange={(e) => set('from', e.target.value)}
      />

      <label>To</label>
      <input
        type="text"
        placeholder="now"
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
