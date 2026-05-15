import { useMemo, useState } from 'react';
import Filters, { localDatetimeToIso, type FilterState } from './components/Filters';
import Map, { type ViewMode } from './components/Map';
import { useLocations, useStats } from './api';

const DEFAULTS: FilterState = {
  user: '',
  device: '',
  from: '',
  to: '',
  limit: 5000,
};

const VIEW_MODES: { value: ViewMode; label: string }[] = [
  { value: 'path', label: 'Path' },
  { value: 'points', label: 'Points' },
  { value: 'heatmap', label: 'Heatmap' },
  { value: 'stops', label: 'Stops & trips' },
];

export default function App() {
  const [filters, setFilters] = useState<FilterState>(DEFAULTS);
  const [viewMode, setViewMode] = useState<ViewMode>('path');
  const stats = useStats();

  const query = useMemo(
    () => ({
      user: filters.user || undefined,
      device: filters.device || undefined,
      from: localDatetimeToIso(filters.from),
      to: localDatetimeToIso(filters.to),
      limit: filters.limit,
      order: 'asc' as const,
    }),
    [filters],
  );

  const locations = useLocations(query);
  const rows = locations.data?.locations ?? [];

  return (
    <div className="app">
      <aside className="sidebar">
        <h1>otrecorder</h1>

        <label>View</label>
        <div className="preset-row">
          {VIEW_MODES.map((m) => (
            <button
              key={m.value}
              type="button"
              className={viewMode === m.value ? 'preset active' : 'preset'}
              onClick={() => setViewMode(m.value)}
            >
              {m.label}
            </button>
          ))}
        </div>

        <Filters value={filters} onChange={setFilters} />
        <div style={{ marginTop: 16, fontSize: 12, opacity: 0.7 }}>
          {stats.data && (
            <>
              <div>{stats.data.total_locations.toLocaleString()} points total</div>
              <div>
                {stats.data.user_count} users · {stats.data.device_count} devices
              </div>
            </>
          )}
        </div>
      </aside>
      <div className="map-wrap">
        <Map locations={rows} viewMode={viewMode} />
        <div className="status">
          {locations.isLoading
            ? 'loading…'
            : locations.isError
              ? `error: ${(locations.error as Error).message}`
              : `${rows.length.toLocaleString()} points`}
        </div>
      </div>
    </div>
  );
}
