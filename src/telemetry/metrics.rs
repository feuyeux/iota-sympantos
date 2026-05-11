use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, UpDownCounter};
use std::sync::OnceLock;

pub struct IotaMetrics {
    pub execution_count: Counter<u64>,
    pub cache_hit_count: Counter<u64>,
    pub cache_miss_count: Counter<u64>,
    pub execution_active: UpDownCounter<i64>,
    pub session_active: UpDownCounter<i64>,
    pub prompt_queued: UpDownCounter<i64>,
    pub token_usage_count: Counter<u64>,
    pub token_input: Counter<u64>,
    pub token_output: Counter<u64>,
    pub token_total: Counter<u64>,
    pub prompt_duration: Histogram<f64>,
    pub init_duration: Histogram<f64>,
}

static METRICS: OnceLock<IotaMetrics> = OnceLock::new();

pub fn get() -> &'static IotaMetrics {
    METRICS.get_or_init(|| {
        let meter = global::meter("iota");
        let buckets = vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];

        IotaMetrics {
            execution_count: meter.u64_counter("iota.execution.count")
                .with_unit("{execution}")
                .with_description("Total execution count by status")
                .build(),
            cache_hit_count: meter.u64_counter("iota.cache.hit.count")
                .with_unit("{hit}")
                .with_description("Cache hit count")
                .build(),
            cache_miss_count: meter.u64_counter("iota.cache.miss.count")
                .with_unit("{miss}")
                .with_description("Cache miss count")
                .build(),
            execution_active: meter.i64_up_down_counter("iota.execution.active")
                .with_unit("{execution}")
                .with_description("Currently running executions")
                .build(),
            session_active: meter.i64_up_down_counter("iota.session.active")
                .with_unit("{session}")
                .with_description("Active sessions")
                .build(),
            prompt_queued: meter.i64_up_down_counter("iota.prompt.queued")
                .with_unit("{prompt}")
                .with_description("Queued prompts")
                .build(),
            token_usage_count: meter.u64_counter("iota.token.usage.count")
                .with_unit("{event}")
                .with_description("Token usage event count")
                .build(),
            token_input: meter.u64_counter("iota.token.input")
                .with_unit("{token}")
                .with_description("Input tokens consumed")
                .build(),
            token_output: meter.u64_counter("iota.token.output")
                .with_unit("{token}")
                .with_description("Output tokens produced")
                .build(),
            token_total: meter.u64_counter("iota.token.total")
                .with_unit("{token}")
                .with_description("Total tokens")
                .build(),
            prompt_duration: meter.f64_histogram("iota.prompt.duration")
                .with_unit("s")
                .with_description("Prompt processing duration")
                .with_boundaries(buckets.clone())
                .build(),
            init_duration: meter.f64_histogram("iota.init.duration")
                .with_unit("s")
                .with_description("ACP initialization duration")
                .with_boundaries(buckets)
                .build(),
        }
    })
}
