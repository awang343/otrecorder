import { useMemo } from 'react';
import { Map as MapGL, Source, Layer, type LayerProps } from 'react-map-gl/maplibre';
import { layers as protomapsLayers, namedFlavor } from '@protomaps/basemaps';
import type {
  LayerSpecification,
  SourceSpecification,
  StyleSpecification,
} from 'maplibre-gl';
import type { LocationRow } from '../types';
import { useTileFiles } from '../api';

const flavor = namedFlavor('light');
const ATTRIBUTION =
  '<a href="https://protomaps.com">Protomaps</a> &copy; <a href="https://openstreetmap.org">OpenStreetMap</a>';

function sourceIdFor(file: string, index: number): string {
  const cleaned = file.replace(/\.pmtiles$/i, '').replace(/[^a-zA-Z0-9_]/g, '_');
  return cleaned ? `pm_${cleaned}` : `pm_${index}`;
}

function buildMapStyle(files: string[]): StyleSpecification {
  const sources: Record<string, SourceSpecification> = {};
  const layers: LayerSpecification[] = [];
  files.forEach((file, i) => {
    const src = sourceIdFor(file, i);
    sources[src] = {
      type: 'vector',
      url: `pmtiles:///tiles/${file}`,
      attribution: ATTRIBUTION,
    };
    const srcLayers = protomapsLayers(src, flavor, { lang: 'en' }) as LayerSpecification[];
    for (const layer of srcLayers) {
      layers.push({ ...layer, id: `${src}__${layer.id}` } as LayerSpecification);
    }
  });
  return {
    version: 8,
    glyphs: 'https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf',
    sprite: 'https://protomaps.github.io/basemaps-assets/sprites/v4/light',
    sources,
    layers,
  };
}

const trackLineLayer: LayerProps = {
  id: 'track-line',
  type: 'line',
  paint: {
    'line-color': '#ff3b30',
    'line-width': 3,
    'line-opacity': 0.85,
  },
  layout: { 'line-cap': 'round', 'line-join': 'round' },
};

const pointsLayer: LayerProps = {
  id: 'track-points',
  type: 'circle',
  paint: {
    'circle-radius': 3,
    'circle-color': '#ff3b30',
    'circle-stroke-color': '#ffffff',
    'circle-stroke-width': 1,
  },
};

type Props = { locations: LocationRow[] };

export default function Map({ locations }: Props) {
  const { data: tileFiles } = useTileFiles();
  const mapStyle = useMemo(() => buildMapStyle(tileFiles ?? []), [tileFiles]);

  const { lineFeature, pointsFC, bounds } = useMemo(() => {
    if (locations.length === 0) {
      return { lineFeature: null, pointsFC: emptyFC(), bounds: null };
    }
    const coords = locations.map((r) => [r.lon, r.lat] as [number, number]);
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
    const lineFC = {
      type: 'Feature' as const,
      geometry: { type: 'LineString' as const, coordinates: coords },
      properties: {},
    };
    const pointsFC = {
      type: 'FeatureCollection' as const,
      features: locations.map((r) => ({
        type: 'Feature' as const,
        geometry: { type: 'Point' as const, coordinates: [r.lon, r.lat] },
        properties: { tst: r.tst, user: r.user, device: r.device },
      })),
    };
    return {
      lineFeature: lineFC,
      pointsFC,
      bounds: [
        [minLon, minLat],
        [maxLon, maxLat],
      ] as [[number, number], [number, number]],
    };
  }, [locations]);

  const initialViewState = bounds
    ? {
        longitude: (bounds[0][0] + bounds[1][0]) / 2,
        latitude: (bounds[0][1] + bounds[1][1]) / 2,
        zoom: 11,
      }
    : { longitude: 0, latitude: 20, zoom: 2 };

  return (
    <MapGL initialViewState={initialViewState} mapStyle={mapStyle} reuseMaps>
      {lineFeature && (
        <Source id="track" type="geojson" data={lineFeature}>
          <Layer {...trackLineLayer} />
        </Source>
      )}
      <Source id="points" type="geojson" data={pointsFC}>
        <Layer {...pointsLayer} />
      </Source>
    </MapGL>
  );
}

function emptyFC() {
  return { type: 'FeatureCollection' as const, features: [] };
}
