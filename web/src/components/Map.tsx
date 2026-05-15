import { useMemo, useState } from 'react';
import {
  Map as MapGL,
  Source,
  Layer,
  Popup,
  type LayerProps,
  type MapLayerMouseEvent,
} from 'react-map-gl/maplibre';
import { layers as protomapsLayers, namedFlavor } from '@protomaps/basemaps';
import type {
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from 'maplibre-gl';
import type { LocationRow } from '../types';
import { useTileFiles } from '../api';

export type ViewMode = 'path' | 'points' | 'heatmap' | 'stops';

const flavor = namedFlavor('light');
const ATTRIBUTION =
  '<a href="https://protomaps.com">Protomaps</a> &copy; <a href="https://openstreetmap.org">OpenStreetMap</a>';

const GAP_TIME_S = 15 * 60;
const GAP_DIST_M = 500;
const STOP_RADIUS_M = 100;
const STOP_MIN_DWELL_S = 10 * 60;

function sourceIdFor(file: string, index: number): string {
  const cleaned = file.replace(/\.pmtiles$/i, '').replace(/[^a-zA-Z0-9_]/g, '_');
  return cleaned ? `pm_${cleaned}` : `pm_${index}`;
}

function buildMapStyle(files: string[]): StyleSpecification {
  const sources: Record<string, SourceSpecification> = {};
  const perSource = files.map((file, i) => {
    const src = sourceIdFor(file, i);
    sources[src] = {
      type: 'vector',
      url: `pmtiles:///tiles/${file}`,
      attribution: ATTRIBUTION,
    };
    return {
      src,
      layers: protomapsLayers(src, flavor, { lang: 'en' }) as LayerSpecification[],
    };
  });

  const layers: LayerSpecification[] = [];
  const slotCount = perSource[0]?.layers.length ?? 0;
  for (let i = 0; i < slotCount; i++) {
    const template = perSource[0].layers[i] as LayerSpecification & { source?: string };
    if (!template.source) {
      layers.push({ ...template, id: `shared__${template.id}` } as LayerSpecification);
      continue;
    }
    for (const { src, layers: sl } of perSource) {
      const layer = sl[i] as LayerSpecification & { id: string };
      layers.push({ ...layer, id: `${src}__${layer.id}` } as LayerSpecification);
    }
  }

  return {
    version: 8,
    glyphs: 'https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf',
    sprite: 'https://protomaps.github.io/basemaps-assets/sprites/v4/light',
    sources,
    layers,
  };
}

// ---------- track analysis ----------

function haversineMeters(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const R = 6_371_000;
  const toRad = Math.PI / 180;
  const dLat = (lat2 - lat1) * toRad;
  const dLon = (lon2 - lon1) * toRad;
  const a =
    Math.sin(dLat / 2) ** 2 +
    Math.cos(lat1 * toRad) * Math.cos(lat2 * toRad) * Math.sin(dLon / 2) ** 2;
  return 2 * R * Math.asin(Math.sqrt(a));
}

type Segment = {
  coords: [number, number][];
  startTst: number;
  endTst: number;
};

function segmentTrack(locations: LocationRow[]): Segment[] {
  const out: Segment[] = [];
  let cur: LocationRow[] = [];
  const flush = () => {
    if (cur.length >= 2) {
      out.push({
        coords: cur.map((r) => [r.lon, r.lat] as [number, number]),
        startTst: cur[0].tst,
        endTst: cur[cur.length - 1].tst,
      });
    }
    cur = [];
  };
  for (const p of locations) {
    if (cur.length > 0) {
      const prev = cur[cur.length - 1];
      if (
        p.tst - prev.tst > GAP_TIME_S ||
        haversineMeters(prev.lat, prev.lon, p.lat, p.lon) > GAP_DIST_M
      ) {
        flush();
      }
    }
    cur.push(p);
  }
  flush();
  return out;
}

type Stop = {
  lon: number;
  lat: number;
  startTst: number;
  endTst: number;
  count: number;
};

function detectStops(locations: LocationRow[]): Stop[] {
  const out: Stop[] = [];
  let cluster: LocationRow[] = [];
  let anchorLat = 0;
  let anchorLon = 0;
  const flush = () => {
    if (cluster.length < 2) return;
    const dur = cluster[cluster.length - 1].tst - cluster[0].tst;
    if (dur < STOP_MIN_DWELL_S) return;
    let lat = 0;
    let lon = 0;
    for (const p of cluster) {
      lat += p.lat;
      lon += p.lon;
    }
    out.push({
      lat: lat / cluster.length,
      lon: lon / cluster.length,
      startTst: cluster[0].tst,
      endTst: cluster[cluster.length - 1].tst,
      count: cluster.length,
    });
  };
  for (const p of locations) {
    if (cluster.length === 0) {
      cluster = [p];
      anchorLat = p.lat;
      anchorLon = p.lon;
      continue;
    }
    if (haversineMeters(anchorLat, anchorLon, p.lat, p.lon) <= STOP_RADIUS_M) {
      cluster.push(p);
    } else {
      flush();
      cluster = [p];
      anchorLat = p.lat;
      anchorLon = p.lon;
    }
  }
  flush();
  return out;
}

function buildTrips(locations: LocationRow[], stops: Stop[]): Segment[] {
  if (locations.length === 0) return [];
  const inStop = new Array<boolean>(locations.length).fill(false);
  let si = 0;
  for (let i = 0; i < locations.length; i++) {
    const t = locations[i].tst;
    while (si < stops.length && t > stops[si].endTst) si++;
    if (si < stops.length && t >= stops[si].startTst && t <= stops[si].endTst) {
      inStop[i] = true;
    }
  }
  const trips: Segment[] = [];
  let cur: LocationRow[] = [];
  const flush = () => {
    if (cur.length >= 2) {
      trips.push({
        coords: cur.map((r) => [r.lon, r.lat] as [number, number]),
        startTst: cur[0].tst,
        endTst: cur[cur.length - 1].tst,
      });
    }
    cur = [];
  };
  for (let i = 0; i < locations.length; i++) {
    if (inStop[i]) {
      flush();
      continue;
    }
    const p = locations[i];
    if (cur.length > 0) {
      const prev = cur[cur.length - 1];
      if (
        p.tst - prev.tst > GAP_TIME_S ||
        haversineMeters(prev.lat, prev.lon, p.lat, p.lon) > GAP_DIST_M
      ) {
        flush();
      }
    }
    cur.push(p);
  }
  flush();
  return trips;
}

// ---------- layer specs ----------

const timeColorExpr = [
  'interpolate',
  ['linear'],
  ['get', 't'],
  0.0,
  '#2563eb',
  0.5,
  '#a855f7',
  1.0,
  '#ef4444',
] as unknown as string;

const segmentLineLayer: LayerProps = {
  id: 'segment-line',
  type: 'line',
  paint: {
    'line-color': timeColorExpr,
    'line-width': 3,
    'line-opacity': 0.85,
  },
  layout: { 'line-cap': 'round', 'line-join': 'round' },
};

const tripLineLayer: LayerProps = {
  id: 'trip-line',
  type: 'line',
  paint: {
    'line-color': timeColorExpr,
    'line-width': 3,
    'line-opacity': 0.85,
  },
  layout: { 'line-cap': 'round', 'line-join': 'round' },
};

const pointsLayer: LayerProps = {
  id: 'track-points',
  type: 'circle',
  paint: {
    'circle-radius': 2.5,
    'circle-color': timeColorExpr,
    'circle-stroke-color': '#ffffff',
    'circle-stroke-width': 0.5,
    'circle-opacity': 0.9,
  },
};

const pointsOnlyLayer: LayerProps = {
  id: 'points-only',
  type: 'circle',
  paint: {
    'circle-radius': ['interpolate', ['linear'], ['zoom'], 0, 2.5, 10, 4.5, 16, 7],
    'circle-color': timeColorExpr,
    'circle-stroke-color': '#ffffff',
    'circle-stroke-width': 1,
    'circle-opacity': 0.9,
  },
};

const heatmapLayer: LayerProps = {
  id: 'heatmap',
  type: 'heatmap',
  paint: {
    'heatmap-weight': 1,
    'heatmap-intensity': ['interpolate', ['linear'], ['zoom'], 0, 1, 15, 3],
    'heatmap-radius': ['interpolate', ['linear'], ['zoom'], 0, 2, 9, 12, 15, 25],
    'heatmap-opacity': 0.75,
    'heatmap-color': [
      'interpolate',
      ['linear'],
      ['heatmap-density'],
      0,
      'rgba(0,0,255,0)',
      0.2,
      '#2563eb',
      0.5,
      '#10b981',
      0.8,
      '#f59e0b',
      1.0,
      '#ef4444',
    ],
  },
};

const stopsLayer: LayerProps = {
  id: 'stops',
  type: 'circle',
  paint: {
    'circle-radius': [
      'interpolate',
      ['linear'],
      ['get', 'duration_s'],
      600,
      6,
      3600,
      10,
      14400,
      16,
      86400,
      24,
    ],
    'circle-color': '#fbbf24',
    'circle-stroke-color': '#111827',
    'circle-stroke-width': 1.5,
    'circle-opacity': 0.9,
  },
};

// ---------- formatting ----------

function fmtTst(tst: number): string {
  return new Date(tst * 1000).toLocaleString();
}

function fmtDuration(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) {
    const h = Math.floor(s / 3600);
    const m = Math.round((s % 3600) / 60);
    return m > 0 ? `${h}h ${m}m` : `${h}h`;
  }
  const d = Math.floor(s / 86400);
  const h = Math.round((s % 86400) / 3600);
  return h > 0 ? `${d}d ${h}h` : `${d}d`;
}

type Hover = {
  lng: number;
  lat: number;
  layerId: string;
  props: Record<string, unknown>;
};

// ---------- component ----------

type Props = { locations: LocationRow[]; viewMode: ViewMode };

export default function Map({ locations, viewMode }: Props) {
  const { data: tileFiles } = useTileFiles();
  const mapStyle = useMemo(() => buildMapStyle(tileFiles ?? []), [tileFiles]);
  const [hover, setHover] = useState<Hover | null>(null);

  const interactiveLayerIds = useMemo(() => {
    if (viewMode === 'path') return ['track-points'];
    if (viewMode === 'points') return ['points-only'];
    if (viewMode === 'stops') return ['stops'];
    return [];
  }, [viewMode]);

  const onMouseMove = (e: MapLayerMouseEvent) => {
    const f = e.features?.[0];
    if (f && f.geometry.type === 'Point') {
      const [lng, lat] = f.geometry.coordinates as [number, number];
      setHover({
        lng,
        lat,
        layerId: f.layer?.id ?? '',
        props: (f.properties ?? {}) as Record<string, unknown>,
      });
    } else if (hover) {
      setHover(null);
    }
  };
  const onMouseLeave = () => setHover(null);

  const data = useMemo(() => {
    if (locations.length === 0) {
      return {
        segmentsFC: emptyFC(),
        pointsFC: emptyFC(),
        stopsFC: emptyFC(),
        tripsFC: emptyFC(),
        bounds: null as null | [[number, number], [number, number]],
      };
    }
    let minLat = locations[0].lat;
    let maxLat = locations[0].lat;
    let minLon = locations[0].lon;
    let maxLon = locations[0].lon;
    for (const r of locations) {
      if (r.lat < minLat) minLat = r.lat;
      if (r.lat > maxLat) maxLat = r.lat;
      if (r.lon < minLon) minLon = r.lon;
      if (r.lon > maxLon) maxLon = r.lon;
    }
    const tFrom = locations[0].tst;
    const tTo = locations[locations.length - 1].tst;
    const span = Math.max(tTo - tFrom, 1);

    const segments = segmentTrack(locations);
    const stops = detectStops(locations);
    const trips = buildTrips(locations, stops);

    return {
      segmentsFC: {
        type: 'FeatureCollection' as const,
        features: segments.map((s) => ({
          type: 'Feature' as const,
          geometry: { type: 'LineString' as const, coordinates: s.coords },
          properties: { t: (s.startTst - tFrom) / span, start: s.startTst, end: s.endTst },
        })),
      },
      pointsFC: {
        type: 'FeatureCollection' as const,
        features: locations.map((r) => ({
          type: 'Feature' as const,
          geometry: { type: 'Point' as const, coordinates: [r.lon, r.lat] },
          properties: {
            tst: r.tst,
            t: (r.tst - tFrom) / span,
            acc: r.acc,
            vel: r.vel,
            alt: r.alt,
          },
        })),
      },
      stopsFC: {
        type: 'FeatureCollection' as const,
        features: stops.map((s) => ({
          type: 'Feature' as const,
          geometry: { type: 'Point' as const, coordinates: [s.lon, s.lat] },
          properties: {
            duration_s: s.endTst - s.startTst,
            start: s.startTst,
            end: s.endTst,
            count: s.count,
          },
        })),
      },
      tripsFC: {
        type: 'FeatureCollection' as const,
        features: trips.map((t) => ({
          type: 'Feature' as const,
          geometry: { type: 'LineString' as const, coordinates: t.coords },
          properties: { t: (t.startTst - tFrom) / span },
        })),
      },
      bounds: [
        [minLon, minLat],
        [maxLon, maxLat],
      ] as [[number, number], [number, number]],
    };
  }, [locations]);

  const initialViewState = data.bounds
    ? {
        longitude: (data.bounds[0][0] + data.bounds[1][0]) / 2,
        latitude: (data.bounds[0][1] + data.bounds[1][1]) / 2,
        zoom: 11,
      }
    : { longitude: 0, latitude: 20, zoom: 2 };

  return (
    <MapGL
      initialViewState={initialViewState}
      mapStyle={mapStyle}
      reuseMaps
      interactiveLayerIds={interactiveLayerIds}
      onMouseMove={onMouseMove}
      onMouseLeave={onMouseLeave}
      cursor={hover ? 'pointer' : 'auto'}
    >
      {viewMode === 'path' && (
        <>
          <Source id="segments" type="geojson" data={data.segmentsFC}>
            <Layer {...segmentLineLayer} />
          </Source>
          <Source id="points" type="geojson" data={data.pointsFC}>
            <Layer {...pointsLayer} />
          </Source>
        </>
      )}
      {viewMode === 'points' && (
        <Source id="points-src" type="geojson" data={data.pointsFC}>
          <Layer {...pointsOnlyLayer} />
        </Source>
      )}
      {viewMode === 'heatmap' && (
        <Source id="heat" type="geojson" data={data.pointsFC}>
          <Layer {...heatmapLayer} />
        </Source>
      )}
      {viewMode === 'stops' && (
        <>
          <Source id="trips" type="geojson" data={data.tripsFC}>
            <Layer {...tripLineLayer} />
          </Source>
          <Source id="stops" type="geojson" data={data.stopsFC}>
            <Layer {...stopsLayer} />
          </Source>
        </>
      )}
      {hover && (
        <Popup
          longitude={hover.lng}
          latitude={hover.lat}
          closeButton={false}
          closeOnClick={false}
          anchor="bottom"
          offset={12}
          className="hover-popup"
        >
          <HoverContent hover={hover} />
        </Popup>
      )}
    </MapGL>
  );
}

function HoverContent({ hover }: { hover: Hover }) {
  const p = hover.props;
  if (hover.layerId === 'stops') {
    const start = Number(p.start);
    const end = Number(p.end);
    const dur = Number(p.duration_s);
    const count = Number(p.count);
    return (
      <div>
        <div className="popup-title">Stop · {fmtDuration(dur)}</div>
        <div className="popup-row">{fmtTst(start)}</div>
        <div className="popup-row">→ {fmtTst(end)}</div>
        <div className="popup-row dim">{count} points</div>
      </div>
    );
  }
  const tst = Number(p.tst);
  const acc = p.acc == null ? null : Number(p.acc);
  const vel = p.vel == null ? null : Number(p.vel);
  const alt = p.alt == null ? null : Number(p.alt);
  return (
    <div>
      <div className="popup-title">{fmtTst(tst)}</div>
      {(acc !== null || vel !== null || alt !== null) && (
        <div className="popup-row dim">
          {acc !== null && <>±{Math.round(acc)} m</>}
          {vel !== null && <> · {Math.round(vel)} km/h</>}
          {alt !== null && <> · {Math.round(alt)} m alt</>}
        </div>
      )}
    </div>
  );
}

function emptyFC() {
  return { type: 'FeatureCollection' as const, features: [] };
}
