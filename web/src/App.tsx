import { useMemo, useState } from 'react';
import Filters, { type FilterState } from './components/Filters';
import Map from './components/Map';
import { useLocations, useStats } from './api';

const DEFAULTS: FilterState = {
  user: '',
  device: '',
  from: '',
  to: '',
  limit: 5000,
};

export default function App() {
  const [filters, setFilters] = useState<FilterState>(DEFAULTS);
  const stats = useStats();

  const query = useMemo(
    () => ({
      user: filters.user || undefined,
      device: filters.device || undefined,
      from: filters.from || undefined,
      to: filters.to || undefined,
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
        <Map locations={rows} />
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
