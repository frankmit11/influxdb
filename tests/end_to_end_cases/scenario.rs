use std::{sync::Arc, time::SystemTime};

use generated_types::google::protobuf::Empty;
use generated_types::influxdata::iox::management::v1::*;
use influxdb_iox_client::flight::PerformQuery;
use rand::{
    distributions::{Alphanumeric, Standard},
    thread_rng, Rng,
};

use std::{convert::TryInto, str, u32};

use futures::prelude::*;
use prost::Message;

use data_types::{names::org_and_bucket_to_database, DatabaseName};
use generated_types::{influxdata::iox::management::v1::DatabaseRules, ReadSource, TimestampRange};

use arrow::{
    array::{ArrayRef, Float64Array, StringArray, TimestampNanosecondArray},
    record_batch::RecordBatch,
};

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T, E = Error> = std::result::Result<T, E>;

/// A test fixture used for working with the influxdb v2 data model
/// (storage gRPC api and v2 write api).
///
/// Each scenario is assigned a a random org and bucket id to ensure
/// tests do not interfere with one another
#[derive(Debug)]
pub struct Scenario {
    org_id: String,
    bucket_id: String,
    ns_since_epoch: i64,
}

impl Scenario {
    /// Create a new `Scenario` with a random org_id and bucket_id
    pub fn new() -> Self {
        let ns_since_epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should have been after the epoch")
            .as_nanos()
            .try_into()
            .expect("Unable to represent system time");

        Self {
            ns_since_epoch,
            org_id: rand_id(),
            bucket_id: rand_id(),
        }
    }

    pub fn org_id_str(&self) -> &str {
        &self.org_id
    }

    pub fn bucket_id_str(&self) -> &str {
        &self.bucket_id
    }

    pub fn org_id(&self) -> u64 {
        u64::from_str_radix(&self.org_id, 16).unwrap()
    }

    pub fn bucket_id(&self) -> u64 {
        u64::from_str_radix(&self.bucket_id, 16).unwrap()
    }

    pub fn database_name(&self) -> DatabaseName<'_> {
        org_and_bucket_to_database(&self.org_id, &self.bucket_id).unwrap()
    }

    pub fn ns_since_epoch(&self) -> i64 {
        self.ns_since_epoch
    }

    pub fn read_source(&self) -> Option<generated_types::google::protobuf::Any> {
        let partition_id = u64::from(u32::MAX);
        let read_source = ReadSource {
            org_id: self.org_id(),
            bucket_id: self.bucket_id(),
            partition_id,
        };

        let mut d = bytes::BytesMut::new();
        read_source.encode(&mut d).unwrap();
        let read_source = generated_types::google::protobuf::Any {
            type_url: "/TODO".to_string(),
            value: d.freeze(),
        };

        Some(read_source)
    }

    pub fn timestamp_range(&self) -> Option<TimestampRange> {
        Some(TimestampRange {
            start: self.ns_since_epoch(),
            end: self.ns_since_epoch() + 10,
        })
    }

    /// Creates the database on the server for this scenario
    pub async fn create_database(&self, client: &mut influxdb_iox_client::management::Client) {
        client
            .create_database(DatabaseRules {
                name: self.database_name().to_string(),
                lifecycle_rules: Some(Default::default()),
                ..Default::default()
            })
            .await
            .unwrap();
    }

    pub async fn load_data(&self, influxdb2: &influxdb2_client::Client) -> Vec<String> {
        // TODO: make a more extensible way to manage data for tests, such as in
        // external fixture files or with factories.
        let points = vec![
            influxdb2_client::models::DataPoint::builder("cpu_load_short")
                .tag("host", "server01")
                .tag("region", "us-west")
                .field("value", 0.64)
                .timestamp(self.ns_since_epoch())
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("cpu_load_short")
                .tag("host", "server01")
                .field("value", 27.99)
                .timestamp(self.ns_since_epoch() + 1)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("cpu_load_short")
                .tag("host", "server02")
                .tag("region", "us-west")
                .field("value", 3.89)
                .timestamp(self.ns_since_epoch() + 2)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("cpu_load_short")
                .tag("host", "server01")
                .tag("region", "us-east")
                .field("value", 1234567.891011)
                .timestamp(self.ns_since_epoch() + 3)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("cpu_load_short")
                .tag("host", "server01")
                .tag("region", "us-west")
                .field("value", 0.000003)
                .timestamp(self.ns_since_epoch() + 4)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("system")
                .tag("host", "server03")
                .field("uptime", 1303385)
                .timestamp(self.ns_since_epoch() + 5)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("swap")
                .tag("host", "server01")
                .tag("name", "disk0")
                .field("in", 3)
                .field("out", 4)
                .timestamp(self.ns_since_epoch() + 6)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("status")
                .field("active", true)
                .timestamp(self.ns_since_epoch() + 7)
                .build()
                .unwrap(),
            influxdb2_client::models::DataPoint::builder("attributes")
                .field("color", "blue")
                .timestamp(self.ns_since_epoch() + 8)
                .build()
                .unwrap(),
        ];
        self.write_data(&influxdb2, points).await.unwrap();

        let host_array = StringArray::from(vec![
            Some("server01"),
            Some("server01"),
            Some("server02"),
            Some("server01"),
            Some("server01"),
        ]);
        let region_array = StringArray::from(vec![
            Some("us-west"),
            None,
            Some("us-west"),
            Some("us-east"),
            Some("us-west"),
        ]);
        let time_array = TimestampNanosecondArray::from_vec(
            vec![
                self.ns_since_epoch,
                self.ns_since_epoch + 1,
                self.ns_since_epoch + 2,
                self.ns_since_epoch + 3,
                self.ns_since_epoch + 4,
            ],
            None,
        );
        let value_array = Float64Array::from(vec![0.64, 27.99, 3.89, 1234567.891011, 0.000003]);

        let batch = RecordBatch::try_from_iter_with_nullable(vec![
            ("host", Arc::new(host_array) as ArrayRef, true),
            ("region", Arc::new(region_array), true),
            ("time", Arc::new(time_array), true),
            ("value", Arc::new(value_array), true),
        ])
        .unwrap();

        arrow::util::pretty::pretty_format_batches(&[batch])
            .unwrap()
            .trim()
            .split('\n')
            .map(|s| s.to_string())
            .collect()
    }

    async fn write_data(
        &self,
        client: &influxdb2_client::Client,
        points: Vec<influxdb2_client::models::DataPoint>,
    ) -> Result<()> {
        client
            .write(
                self.org_id_str(),
                self.bucket_id_str(),
                stream::iter(points),
            )
            .await?;
        Ok(())
    }
}

/// substitutes "ns" --> ns_since_epoch, ns1-->ns_since_epoch+1, etc
pub fn substitute_nanos(ns_since_epoch: i64, lines: &[&str]) -> Vec<String> {
    let substitutions = vec![
        ("ns0", format!("{}", ns_since_epoch)),
        ("ns1", format!("{}", ns_since_epoch + 1)),
        ("ns2", format!("{}", ns_since_epoch + 2)),
        ("ns3", format!("{}", ns_since_epoch + 3)),
        ("ns4", format!("{}", ns_since_epoch + 4)),
        ("ns5", format!("{}", ns_since_epoch + 5)),
        ("ns6", format!("{}", ns_since_epoch + 6)),
    ];

    lines
        .iter()
        .map(|line| {
            let mut line = line.to_string();
            for (from, to) in &substitutions {
                line = line.replace(from, to);
            }
            line
        })
        .collect()
}

/// Return a random string suitable for use as a database name
pub fn rand_name() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

// return a random 16 digit string comprised of numbers suitable for
// use as a influxdb2 org_id or bucket_id
pub fn rand_id() -> String {
    thread_rng()
        .sample_iter(&Standard)
        .filter_map(|c: u8| {
            if c.is_ascii_digit() {
                Some(char::from(c))
            } else {
                // discard if out of range
                None
            }
        })
        .take(16)
        .collect()
}

/// given a channel to talk with the management api, create a new
/// database with the specified name configured with a 10MB mutable
/// buffer, partitioned on table
pub async fn create_readable_database(
    db_name: impl Into<String>,
    channel: tonic::transport::Channel,
) {
    create_readable_database_plus(db_name, channel, std::convert::identity).await
}

/// given a channel to talk with the management api, create a new
/// database with the specified name configured with a 10MB mutable
/// buffer, partitioned on table
pub async fn create_readable_database_plus(
    db_name: impl Into<String>,
    channel: tonic::transport::Channel,
    modify_rules: impl FnOnce(DatabaseRules) -> DatabaseRules,
) {
    let mut management_client = influxdb_iox_client::management::Client::new(channel);

    let rules = DatabaseRules {
        name: db_name.into(),
        partition_template: Some(PartitionTemplate {
            parts: vec![partition_template::Part {
                part: Some(partition_template::part::Part::Table(Empty {})),
            }],
        }),
        lifecycle_rules: Some(LifecycleRules {
            buffer_size_hard: 1024 * 1024,
            ..Default::default()
        }),
        ..Default::default()
    };

    let rules = modify_rules(rules);

    management_client
        .create_database(rules)
        .await
        .expect("create database failed");
}

/// given a channel to talk with the management api, create a new
/// database with the specified name that will aggressively try and
/// persist all data quickly
pub async fn create_quickly_persisting_database(
    db_name: impl Into<String>,
    channel: tonic::transport::Channel,
) {
    let db_name = db_name.into();

    let mut management_client = influxdb_iox_client::management::Client::new(channel);
    let rules = DatabaseRules {
        name: db_name.clone(),
        partition_template: Some(PartitionTemplate {
            parts: vec![partition_template::Part {
                part: Some(partition_template::part::Part::Time(
                    "%Y-%m-%d %H:00:00".into(),
                )),
            }],
        }),
        lifecycle_rules: Some(LifecycleRules {
            mutable_linger_seconds: 1,
            mutable_size_threshold: 100,
            buffer_size_soft: 512 * 1024,  // 512K
            buffer_size_hard: 1024 * 1024, // 1MB
            persist: true,
            ..Default::default()
        }),
        ..Default::default()
    };

    management_client
        .create_database(rules.clone())
        .await
        .expect("create database failed");
    println!("Created quickly persisting database {}", db_name);
}

/// given a channel to talk with the managment api, create a new
/// database with no mutable buffer configured, no partitioning rules
pub async fn create_unreadable_database(
    db_name: impl Into<String>,
    channel: tonic::transport::Channel,
) {
    let mut management_client = influxdb_iox_client::management::Client::new(channel);

    let rules = DatabaseRules {
        name: db_name.into(),
        ..Default::default()
    };

    management_client
        .create_database(rules.clone())
        .await
        .expect("create database failed");
}

/// given a channel to talk with the managment api, create a new
/// database with the specified name configured with a 10MB mutable
/// buffer, partitioned on table, with some data written into two partitions
pub async fn create_two_partition_database(
    db_name: impl Into<String>,
    channel: tonic::transport::Channel,
) {
    let mut write_client = influxdb_iox_client::write::Client::new(channel.clone());

    let db_name = db_name.into();
    create_readable_database(&db_name, channel).await;

    let lp_lines = vec![
        "mem,host=foo free=27875999744i,cached=0i,available_percent=62.2 1591894320000000000",
        "cpu,host=foo running=4i,sleeping=514i,total=519i 1592894310000000000",
    ];

    write_client
        .write(&db_name, lp_lines.join("\n"))
        .await
        .expect("write succeded");
}

/// Collect the results of a query into a vector of record batches
pub async fn collect_query(mut query_results: PerformQuery) -> Vec<RecordBatch> {
    let mut batches = vec![];
    while let Some(data) = query_results.next().await.unwrap() {
        batches.push(data);
    }
    batches
}
