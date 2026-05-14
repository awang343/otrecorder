import { useQuery } from '@tanstack/react-query';
import type {
  BboxQuery,
  DevicesResponse,
  LocationsQuery,
  LocationsResponse,
  Stats,
  UsersResponse,
} from './types';

function qs(params: Record<string, unknown>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue;
    u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path);
  if (!r.ok) {
    let detail = '';
    try {
      detail = (await r.json()).error ?? '';
    } catch {
      detail = await r.text();
    }
    throw new Error(`${r.status} ${r.statusText}${detail ? `: ${detail}` : ''}`);
  }
  return r.json() as Promise<T>;
}

export const useStats = () =>
  useQuery({ queryKey: ['stats'], queryFn: () => get<Stats>('/api/stats') });

export const useUsers = () =>
  useQuery({ queryKey: ['users'], queryFn: () => get<UsersResponse>('/api/users') });

export const useDevices = (user?: string) =>
  useQuery({
    queryKey: ['devices', user ?? null],
    queryFn: () =>
      get<DevicesResponse>(user ? `/api/users/${encodeURIComponent(user)}/devices` : '/api/devices'),
  });

export const useLocations = (q: LocationsQuery, enabled = true) =>
  useQuery({
    queryKey: ['locations', q],
    queryFn: () => get<LocationsResponse>(`/api/locations${qs(q)}`),
    enabled,
  });

export const useLatest = (user?: string, device?: string) =>
  useQuery({
    queryKey: ['latest', user ?? null, device ?? null],
    queryFn: () => get<LocationsResponse>(`/api/locations/latest${qs({ user, device })}`),
  });

export const useBbox = (q: BboxQuery, enabled = true) =>
  useQuery({
    queryKey: ['bbox', q],
    queryFn: () => get<LocationsResponse>(`/api/locations/bbox${qs(q)}`),
    enabled,
  });
