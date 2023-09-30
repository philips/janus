use common::{submit_measurements_and_verify_aggregate, test_task_builder};
use janus_aggregator_core::task::QueryType;
use janus_core::{
    test_util::{install_test_trace_subscriber, testcontainers::container_client},
    vdaf::VdafInstance,
};
use janus_integration_tests::{client::ClientBackend, janus::Janus, TaskParameters};
use janus_interop_binaries::test_util::generate_network_name;
use janus_messages::Role;
use testcontainers::clients::Cli;

mod common;

/// A pair of Janus instances, running in containers, against which integration tests may be run.
struct JanusPair<'a> {
    /// Task parameters needed by the client and collector, for the task configured in both Janus
    /// aggregators.
    task_parameters: TaskParameters,

    /// Handle to the leader's resources, which are released on drop.
    leader: Janus<'a>,
    /// Handle to the helper's resources, which are released on drop.
    helper: Janus<'a>,
}

impl<'a> JanusPair<'a> {
    /// Set up a new pair of containerized Janus test instances, and set up a new task in each using
    /// the given VDAF and query type.
    pub async fn new(
        test_name: &str,
        container_client: &'a Cli,
        vdaf: VdafInstance,
        query_type: QueryType,
    ) -> JanusPair<'a> {
        let (task_parameters, task_builder) = test_task_builder(vdaf, query_type);
        let task = task_builder.build();

        let network = generate_network_name();
        let leader = Janus::new(test_name, container_client, &network, &task, Role::Leader).await;
        let helper = Janus::new(test_name, container_client, &network, &task, Role::Helper).await;

        Self {
            task_parameters,
            leader,
            helper,
        }
    }
}

/// This test exercises Prio3Count with Janus as both the leader and the helper.
#[tokio::test(flavor = "multi_thread")]
async fn janus_janus_count() {
    static TEST_NAME: &str = "janus_janus_count";
    install_test_trace_subscriber();

    // Start servers.
    let container_client = container_client();
    let janus_pair = JanusPair::new(
        TEST_NAME,
        &container_client,
        VdafInstance::Prio3Count,
        QueryType::TimeInterval,
    )
    .await;

    // Run the behavioral test.
    submit_measurements_and_verify_aggregate(
        TEST_NAME,
        &janus_pair.task_parameters,
        (janus_pair.leader.port(), janus_pair.helper.port()),
        &ClientBackend::InProcess,
    )
    .await;
}

/// This test exercises Prio3Sum with Janus as both the leader and the helper.
#[tokio::test(flavor = "multi_thread")]
async fn janus_janus_sum_16() {
    static TEST_NAME: &str = "janus_janus_sum_16";
    install_test_trace_subscriber();

    // Start servers.
    let container_client = container_client();
    let janus_pair = JanusPair::new(
        TEST_NAME,
        &container_client,
        VdafInstance::Prio3Sum { bits: 16 },
        QueryType::TimeInterval,
    )
    .await;

    // Run the behavioral test.
    submit_measurements_and_verify_aggregate(
        TEST_NAME,
        &janus_pair.task_parameters,
        (janus_pair.leader.port(), janus_pair.helper.port()),
        &ClientBackend::InProcess,
    )
    .await;
}

/// This test exercises Prio3Histogram with Janus as both the leader and the helper.
#[tokio::test(flavor = "multi_thread")]
async fn janus_janus_histogram_4_buckets() {
    static TEST_NAME: &str = "janus_janus_histogram_4_buckets";
    install_test_trace_subscriber();

    // Start servers.
    let container_client = container_client();
    let janus_pair = JanusPair::new(
        TEST_NAME,
        &container_client,
        VdafInstance::Prio3Histogram {
            length: 4,
            chunk_length: 2,
        },
        QueryType::TimeInterval,
    )
    .await;

    // Run the behavioral test.
    submit_measurements_and_verify_aggregate(
        TEST_NAME,
        &janus_pair.task_parameters,
        (janus_pair.leader.port(), janus_pair.helper.port()),
        &ClientBackend::InProcess,
    )
    .await;
}

/// This test exercises the fixed-size query type with Janus as both the leader and the helper.
#[tokio::test(flavor = "multi_thread")]
async fn janus_janus_fixed_size() {
    static TEST_NAME: &str = "janus_janus_fixed_size";
    install_test_trace_subscriber();

    // Start servers.
    let container_client = container_client();
    let janus_pair = JanusPair::new(
        TEST_NAME,
        &container_client,
        VdafInstance::Prio3Count,
        QueryType::FixedSize {
            max_batch_size: 50,
            batch_time_window_size: None,
        },
    )
    .await;

    // Run the behavioral test.
    submit_measurements_and_verify_aggregate(
        TEST_NAME,
        &janus_pair.task_parameters,
        (janus_pair.leader.port(), janus_pair.helper.port()),
        &ClientBackend::InProcess,
    )
    .await;
}

// This test exercises Prio3SumVec with Janus as both the leader and the helper.
#[tokio::test(flavor = "multi_thread")]
async fn janus_janus_sum_vec() {
    static TEST_NAME: &str = "janus_janus_sum_vec";
    install_test_trace_subscriber();

    let container_client = container_client();
    let janus_pair = JanusPair::new(
        TEST_NAME,
        &container_client,
        VdafInstance::Prio3SumVec {
            bits: 16,
            length: 15,
            chunk_length: 16,
        },
        QueryType::TimeInterval,
    )
    .await;

    submit_measurements_and_verify_aggregate(
        TEST_NAME,
        &janus_pair.task_parameters,
        (janus_pair.leader.port(), janus_pair.helper.port()),
        &ClientBackend::InProcess,
    )
    .await;
}
