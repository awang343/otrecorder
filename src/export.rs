use anyhow::{Context, Result};
use arrow_array::{
    builder::{Float64Builder, Int64Builder, StringBuilder},
    ArrayRef, RecordBatch,
};
use arrow_schema::{DataType, Field, Schema};
use parquet::{
    arrow::ArrowWriter,
    basic::{Compression, ZstdLevel},
    file::properties::WriterProperties,
};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

use crate::storage::{LocationFilter, LocationRow, Storage};

pub fn export_parquet(
    storage: &Storage,
    out_path: &Path,
    filter: &LocationFilter,
    batch_size: usize,
) -> Result<u64> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create out parent dir {}", parent.display()))?;
        }
    }
    let schema = Arc::new(build_schema());
    let file = File::create(out_path)
        .with_context(|| format!("create output file {}", out_path.display()))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

    let mut batch = RowBatchBuilder::new(batch_size);
    let mut total: u64 = 0;
    let total_ref = &mut total;
    let writer_ref = &mut writer;
    let schema_ref = schema.clone();

    let written = storage.stream_locations(filter, |row| {
        batch.push(row);
        if batch.len() >= batch_size {
            let rb = batch.finish(&schema_ref)?;
            writer_ref.write(&rb)?;
            *total_ref += rb.num_rows() as u64;
        }
        Ok(())
    })?;

    if batch.len() > 0 {
        let rb = batch.finish(&schema)?;
        writer.write(&rb)?;
        total += rb.num_rows() as u64;
    }
    writer.close()?;

    info!(rows = total, source_rows = written, out = %out_path.display(), "parquet export done");
    Ok(total)
}

fn build_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("topic", DataType::Utf8, false),
        Field::new("user", DataType::Utf8, false),
        Field::new("device", DataType::Utf8, false),
        Field::new("tst", DataType::Int64, false),
        Field::new("received_at", DataType::Int64, false),
        Field::new("lat", DataType::Float64, false),
        Field::new("lon", DataType::Float64, false),
        Field::new("acc", DataType::Float64, true),
        Field::new("alt", DataType::Float64, true),
        Field::new("vel", DataType::Float64, true),
        Field::new("cog", DataType::Float64, true),
        Field::new("batt", DataType::Float64, true),
        Field::new("bs", DataType::Int64, true),
        Field::new("trigger", DataType::Utf8, true),
        Field::new("tid", DataType::Utf8, true),
        Field::new("conn", DataType::Utf8, true),
        Field::new("vac", DataType::Float64, true),
        Field::new("pressure", DataType::Float64, true),
    ])
}

struct RowBatchBuilder {
    id: Int64Builder,
    topic: StringBuilder,
    user: StringBuilder,
    device: StringBuilder,
    tst: Int64Builder,
    received_at: Int64Builder,
    lat: Float64Builder,
    lon: Float64Builder,
    acc: Float64Builder,
    alt: Float64Builder,
    vel: Float64Builder,
    cog: Float64Builder,
    batt: Float64Builder,
    bs: Int64Builder,
    trigger: StringBuilder,
    tid: StringBuilder,
    conn: StringBuilder,
    vac: Float64Builder,
    pressure: Float64Builder,
    len: usize,
}

impl RowBatchBuilder {
    fn new(cap: usize) -> Self {
        Self {
            id: Int64Builder::with_capacity(cap),
            topic: StringBuilder::with_capacity(cap, cap * 32),
            user: StringBuilder::with_capacity(cap, cap * 8),
            device: StringBuilder::with_capacity(cap, cap * 8),
            tst: Int64Builder::with_capacity(cap),
            received_at: Int64Builder::with_capacity(cap),
            lat: Float64Builder::with_capacity(cap),
            lon: Float64Builder::with_capacity(cap),
            acc: Float64Builder::with_capacity(cap),
            alt: Float64Builder::with_capacity(cap),
            vel: Float64Builder::with_capacity(cap),
            cog: Float64Builder::with_capacity(cap),
            batt: Float64Builder::with_capacity(cap),
            bs: Int64Builder::with_capacity(cap),
            trigger: StringBuilder::with_capacity(cap, cap * 2),
            tid: StringBuilder::with_capacity(cap, cap * 4),
            conn: StringBuilder::with_capacity(cap, cap * 2),
            vac: Float64Builder::with_capacity(cap),
            pressure: Float64Builder::with_capacity(cap),
            len: 0,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn push(&mut self, r: LocationRow) {
        self.id.append_value(r.id);
        self.topic.append_value(&r.topic);
        self.user.append_value(&r.user);
        self.device.append_value(&r.device);
        self.tst.append_value(r.tst);
        self.received_at.append_value(r.received_at);
        self.lat.append_value(r.lat);
        self.lon.append_value(r.lon);
        push_opt_f64(&mut self.acc, r.acc);
        push_opt_f64(&mut self.alt, r.alt);
        push_opt_f64(&mut self.vel, r.vel);
        push_opt_f64(&mut self.cog, r.cog);
        push_opt_f64(&mut self.batt, r.batt);
        match r.bs {
            Some(v) => self.bs.append_value(v),
            None => self.bs.append_null(),
        }
        push_opt_str(&mut self.trigger, r.trigger.as_deref());
        push_opt_str(&mut self.tid, r.tid.as_deref());
        push_opt_str(&mut self.conn, r.conn.as_deref());
        push_opt_f64(&mut self.vac, r.vac);
        push_opt_f64(&mut self.pressure, r.pressure);
        self.len += 1;
    }

    fn finish(&mut self, schema: &Arc<Schema>) -> Result<RecordBatch> {
        let arrays: Vec<ArrayRef> = vec![
            Arc::new(self.id.finish()),
            Arc::new(self.topic.finish()),
            Arc::new(self.user.finish()),
            Arc::new(self.device.finish()),
            Arc::new(self.tst.finish()),
            Arc::new(self.received_at.finish()),
            Arc::new(self.lat.finish()),
            Arc::new(self.lon.finish()),
            Arc::new(self.acc.finish()),
            Arc::new(self.alt.finish()),
            Arc::new(self.vel.finish()),
            Arc::new(self.cog.finish()),
            Arc::new(self.batt.finish()),
            Arc::new(self.bs.finish()),
            Arc::new(self.trigger.finish()),
            Arc::new(self.tid.finish()),
            Arc::new(self.conn.finish()),
            Arc::new(self.vac.finish()),
            Arc::new(self.pressure.finish()),
        ];
        let rb = RecordBatch::try_new(schema.clone(), arrays)?;
        self.len = 0;
        Ok(rb)
    }
}

fn push_opt_f64(b: &mut Float64Builder, v: Option<f64>) {
    match v {
        Some(x) => b.append_value(x),
        None => b.append_null(),
    }
}
fn push_opt_str(b: &mut StringBuilder, v: Option<&str>) {
    match v {
        Some(x) => b.append_value(x),
        None => b.append_null(),
    }
}
