use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tracing::{error, warn};
use utoipa::ToSchema;

use std::{
    fmt::Display,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "start_time": 1514764800,
    "end_time": 1674864000
}))]
pub struct BatchJobRequest {
    start_time: i64,
    end_time: i64,
}

#[derive(Serialize, ToSchema)]
pub struct BatchJobResponse {
    job_id: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum BatchJobStatus {
    Pending,
    InProgress,
    Completed,
}

impl Display for BatchJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchJobStatus::Pending => write!(f, "pending"),
            BatchJobStatus::InProgress => write!(f, "in_progress"),
            BatchJobStatus::Completed => write!(f, "completed"),
        }
    }
}

// used by the batch_job endpoint
// verifies:
//  1. start time is after defi started
//  2. end time is not in the future
//  3. start time is lower than end time
fn is_valid_time_range(start_time: i64, end_time: i64) -> bool {
    const DEFI_START: i64 = 1514764800; // 2018-01-01
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    start_time >= DEFI_START && end_time <= now && start_time < end_time
}

#[utoipa::path(
    post,
    path = "/v1/jobs",
    request_body = BatchJobRequest,
    responses(
        (status = 201, description = "Batch job created", body = BatchJobResponse),
        (status = 400, description = "Invalid time range"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn create_batch_job(
    db_pool: web::Data<PgPool>,
    redis: web::Data<redis::Client>,
    req: web::Json<BatchJobRequest>,
) -> HttpResponse {
    if !is_valid_time_range(req.start_time, req.end_time) {
        return HttpResponse::BadRequest()
            .json(json!({"error": "Invalid time range. Must be between 2018-01-01 and now"}));
    }

    let job_id = match sqlx::query!(
        "INSERT INTO batch_jobs (start_time, end_time, status) VALUES ($1, $2, $3) RETURNING id",
        req.start_time,
        req.end_time,
        BatchJobStatus::Pending.to_string()
    )
    .fetch_one(db_pool.get_ref())
    .await
    {
        Ok(row) => row.id,
        Err(e) => {
            error!(
                error = ?e,
                sql_query = "INSERT INTO batch_jobs",
                "Database error occurred"
            );
            return HttpResponse::InternalServerError().finish();
        }
    };

    let mut conn = match redis.get_ref().get_multiplexed_async_connection().await {
        Ok(conn) => conn,
        Err(e) => {
            error!(
                error = ?e,
                job_id = job_id,
                "Failed to acquire Redis connection"
            );
            return HttpResponse::InternalServerError().finish();
        }
    };

    if let Err(e) = redis::cmd("RPUSH")
        .arg("batch_jobs")
        .arg(job_id)
        .query_async::<()>(&mut conn)
        .await
    {
        error!(
            error = ?e,
            job_id = job_id,
            "Failed to push job to Redis"
        );
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Created().json(BatchJobResponse { job_id })
}

#[derive(Serialize, ToSchema)]
pub struct BatchJobStatusResponse {
    job_id: i64,
    status: BatchJobStatus,
    start_time: i64,
    end_time: i64,
}

#[utoipa::path(
    get,
    path = "/v1/jobs/{job_id}",
    params(
        ("job_id" = i64, Path, description = "Batch job ID")
    ),
    responses(
        (status = 200, description = "Job status retrieved", body = BatchJobStatusResponse),
        (status = 404, description = "Job not found"),
        (status = 500, description = "Internal server error"),
    )
)]
pub async fn get_job_status(db_pool: web::Data<PgPool>, job_id: web::Path<i64>) -> HttpResponse {
    let job_id = job_id.into_inner();

    match sqlx::query!(
        "SELECT id, status, start_time, end_time FROM batch_jobs WHERE id = $1",
        job_id
    )
    .fetch_optional(db_pool.get_ref())
    .await
    {
        Ok(Some(job)) => {
            let status = match job.status.as_str() {
                "pending" => BatchJobStatus::Pending,
                "in_progress" => BatchJobStatus::InProgress,
                "completed" => BatchJobStatus::Completed,
                _ => {
                    error!(
                        job_id = job_id,
                        status = ?job.status,
                        "Invalid job status in database"
                    );
                    return HttpResponse::InternalServerError().finish();
                }
            };

            HttpResponse::Ok().json(BatchJobStatusResponse {
                job_id: job.id,
                status,
                start_time: job.start_time,
                end_time: job.end_time,
            })
        }
        Ok(None) => {
            warn!(job_id = job_id, "Job not found");
            HttpResponse::NotFound().finish()
        }
        Err(e) => {
            error!(
                error = ?e,
                job_id = job_id,
                "Database error while fetching job status"
            );
            HttpResponse::InternalServerError().finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_time_range() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let cases = vec![
            (1514764800, now, true),         // DeFi start to now - valid
            (1514764799, now, false),        // before DeFi start - invalid
            (1514764800, 1514764801, true),  // minimal valid range
            (now + 1, now + 2, false),       // future range - invalid
            (1514764800, 1514764800, false), // same timestamps - invalid
            (1514764801, 1514764800, false), // end before start - invalid
        ];

        for (start, end, expected) in cases {
            assert_eq!(
                is_valid_time_range(start, end),
                expected,
                "Failed for start: {}, end: {}",
                start,
                end
            );
        }
    }
}
