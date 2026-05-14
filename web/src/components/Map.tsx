import { useMemo } from 'react';
import { Map as MapGL, Source, Layer, type LayerProps } from 'react-map-gl/maplibre';
import { layers as protomapsLayers, namedFlavor } from '@protomaps/basemaps';
import type { LocationRow } from '../types';

const PMTILES_URL = '/tiles/map.pmtiles';

const flavor = namedFlavor('light');

const mapStyle = {
  version: 8 as const,
  glyphs: 'https://protomaps.github.io/basemaps-assets/fonts/{fontstack}/{range}.pbf',
  sprite: 'https://protomaps.github.io/basemaps-assets/sprites/v4/light',
  sources: {
    protomaps: {
      type: 'vector' as const,
      url: `pmtiles://${PMTILES_URL}`,
      attribution:
        '<a href="https://protomaps.com">Protomaps</a> &copy; <a href="https://openstreetmap.org">OpenStreetMap</a>',
    },
  },
  layers: protomapsLayers('protomaps', flavor, { lang: 'en' }),
};

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
