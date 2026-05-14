export type LocationRow = {
  id: number;
  topic: string;
  user: string;
  device: string;
  tst: number;
  received_at: number;
  lat: number;
  lon: number;
  acc: number | null;
  alt: number | null;
  vel: number | null;
  cog: number | null;
  batt: number | null;
  bs: number | null;
  trigger: string | null;
  tid: string | null;
  conn: string | null;
  vac: number | null;
  pressure: number | null;
};

export type UserSummary = {
  user: string;
  device_count: number;
  location_count: number;
};

export type DeviceSummary = {
  user: string;
  device: string;
  location_count: number;
  first_tst: number | null;
  last_tst: number | null;
};

export type Stats = {
  total_locations: number;
  total_messages: number;
  oldest_tst: number | null;
  newest_tst: number | null;
  user_count: number;
  device_count: number;
  db_size_bytes: number;
};

export type LocationsResponse = { count: number; locations: LocationRow[] };
export type UsersResponse = { users: UserSummary[] };
export type DevicesResponse = { devices: DeviceSummary[] };

export type Order = 'asc' | 'desc';

export type LocationsQuery = {
  user?: string;
  device?: string;
  from?: number | string;
  to?: number | string;
  limit?: number;
  offset?: number;
  order?: Order;
};

export type BboxQuery = LocationsQuery & {
  min_lat: number;
  max_lat: number;
  min_lon: number;
  max_lon: number;
};
