# Docker Deployment Guide

This guide explains how to deploy the email service using Docker in various cloud environments.

## Prerequisites

- Docker installed (20.10 or later)
- Docker Compose installed (for docker-compose deployment)
- A 64-character hex string for BLAKE3_KEY
- SMTP credentials and an admin API key (see `.env.sample`)

## Quick Start with Docker Compose

1. **Clone the repository:**
   ```bash
   git clone <repository-url>
   cd email-serv
   ```

2. **Generate a secure BLAKE3_KEY:**
   ```bash
   # Generate 64 random hex characters (32 bytes)
   openssl rand -hex 32
   ```

3. **Create a `.env` file** (Docker Compose reads it automatically for the
   `${VAR}` references in `docker-compose.yml`):
   ```bash
   cp .env.sample .env
   # then edit .env and set at least:
   #   BLAKE3_KEY, SMTP_HOST, SMTP_PORT, SMTP_USER, SMTP_PASS,
   #   EMAIL_FROM, ADMIN_API_KEY and SITE_URL
   ```

4. **Run the service:**
   ```bash
   docker-compose up -d
   ```

5. **Verify deployment:**
   ```bash
   curl http://localhost:8080/health_check
   ```

## Build and Run with Docker

### Build the image:
```bash
docker build -t email-serv:latest .
```

### Run the container:
```bash
docker run -d \
  --name email-serv \
  -p 8080:8080 \
  -e BLAKE3_KEY=<your-generated-key> \
  -e SMTP_HOST=smtp.example.com \
  -e SMTP_PORT=587 \
  -e SMTP_USER=user@example.com \
  -e SMTP_PASS=<your-smtp-password> \
  -e EMAIL_FROM=noreply@example.com \
  -e ADMIN_API_KEY=<your-admin-key> \
  -e SITE_URL=https://example.com \
  -v email_data:/app/data \
  -v email_logs:/app/logs \
  email-serv:latest
```

### View logs:
```bash
docker logs -f email-serv
```

## Cloud Deployment

### AWS ECS/Fargate

1. **Push image to ECR:**
   ```bash
   # Login to ECR
   aws ecr get-login-password --region us-east-1 | docker login --username AWS --password-stdin <account-id>.dkr.ecr.us-east-1.amazonaws.com
   
   # Tag image
   docker tag email-serv:latest <account-id>.dkr.ecr.us-east-1.amazonaws.com/email-serv:latest
   
   # Push image
   docker push <account-id>.dkr.ecr.us-east-1.amazonaws.com/email-serv:latest
   ```

2. **Create ECS task definition** with environment variables:
   ```json
   {
     "containerDefinitions": [
       {
         "name": "email-serv",
         "image": "<account-id>.dkr.ecr.us-east-1.amazonaws.com/email-serv:latest",
         "memory": 512,
         "cpu": 256,
         "portMappings": [
           {
             "containerPort": 8080
           }
         ],
         "environment": [
           {
             "name": "BLAKE3_KEY",
             "value": "<your-secret-key>"
           },
           {
             "name": "RUST_LOG",
             "value": "email-serv=info"
           }
         ],
         "mountPoints": [
           {
             "sourceVolume": "email-data",
             "containerPath": "/app/data"
           }
         ]
       }
     ],
     "volumes": [
       {
         "name": "email-data",
         "efsVolumeConfiguration": {
           "fileSystemId": "<your-efs-id>",
           "rootDirectory": "/email-serv"
         }
       }
     ]
   }
   ```

3. **Create ECS service** and configure ALB health check at `/health_check`

### Google Cloud Run

1. **Push image to GCR:**
   ```bash
   # Configure Docker for GCR
   gcloud auth configure-docker us-central1-docker.pkg.dev
   
   # Tag and push
   docker tag email-serv:latest us-central1-docker.pkg.dev/<project-id>/email-serv:latest
   docker push us-central1-docker.pkg.dev/<project-id>/email-serv:latest
   ```

2. **Deploy to Cloud Run:**
   ```bash
   gcloud run deploy email-serv \
     --image=us-central1-docker.pkg.dev/<project-id>/email-serv:latest \
     --platform=managed \
     --region=us-central1 \
     --allow-unauthenticated \
     --port=8080 \
     --memory=512Mi \
     --cpu=1 \
     --set-env-vars="BLAKE3_KEY=<your-secret-key>,RUST_LOG=email-serv=info" \
     --set-cloudsql-instances=<cloudsql-connection-string> \
     --health-check-path=/health_check \
     --timeout=300
   ```

**Note:** For production with Cloud SQL, replace local SQLite with Cloud SQL PostgreSQL/MySQL.

### Azure Container Instances

1. **Push image to ACR:**
   ```bash
   # Login to ACR
   az acr login --name <acr-name>
   
   # Tag and push
   docker tag email-serv:latest <acr-name>.azurecr.io/email-serv:latest
   docker push <acr-name>.azurecr.io/email-serv:latest
   ```

2. **Deploy container instance:**
   ```bash
   az container create \
     --resource-group <rg-name> \
     --name email-serv \
     --image <acr-name>.azurecr.io/email-serv:latest \
     --cpu 1 \
     --memory 0.5 \
     --ports 8080 \
     --environment-variables "BLAKE3_KEY=<your-secret-key>" \
     --azure-file-volume-share email-data /app/data \
     --protocol TCP \
     --restart-policy Always
   ```

### DigitalOcean App Platform

1. **Push image to DigitalOcean Container Registry (DOCR):**
   ```bash
   # Login to DOCR
   doctl registry login
   
   # Tag and push
   docker tag email-serv:latest registry.digitalocean.com/<namespace>/email-serv:latest
   docker push registry.digitalocean.com/<namespace>/email-serv:latest
   ```

2. **Create app with CLI:**
   ```bash
   doctl apps create --spec .do/app-spec.yaml
   ```

**Example `.do/app-spec.yaml`:**
```yaml
name: email-serv
services:
- name: web
  image:
    registry_type: DOCR
    registry: <namespace>/email-serv
    deploy_on_push: true
  http_port: 8080
  instance_size_slug: basic-xxs
  instance_count: 1
  envs:
  - key: BLAKE3_KEY
    value: <your-secret-key>
    scope: RUN_TIME
  - key: RUST_LOG
    value: email-serv=info
    scope: RUN_TIME
  volumes:
  - name: email-data
    mount_path: /app/data
    size_gb: 1
```

## Environment Variables

| Variable | Required | Default | Description |
|-----------|-----------|----------|-------------|
| `BLAKE3_KEY` | Yes | - | 64-character hex string for keyed email hashing |
| `SMTP_HOST` | Yes | - | SMTP relay host (e.g. `smtp.example.com`) |
| `SMTP_PORT` | Yes | - | SMTP port (587 for STARTTLS, 465 for implicit TLS) |
| `SMTP_USER` | Yes | - | SMTP username |
| `SMTP_PASS` | Yes | - | SMTP password |
| `EMAIL_FROM` | Yes | - | From address for outgoing mail |
| `ADMIN_API_KEY` | Yes | - | Pre-shared key for `POST /api/admin/broadcast` |
| `SITE_URL` | No | `http://localhost:3000` | Public base URL used in verification/unsubscribe links |
| `DB_CONN` | No | `/app/data/subscribers.db` | SQLite database path |
| `PORT` | No | `8080` | HTTP server port |
| `LOG_DIR` | No | `/app/logs` | Log directory path |
| `RUST_LOG` | No | `email-serv=info` | Logging level |

## Volumes

### `/app/data` - Database Storage
- **Purpose**: Persistent SQLite database storage
- **Recommendation**: Use network-attached storage for production
  - AWS: EFS or EBS
  - GCP: Filestore or Persistent Disk
  - Azure: Azure Files
  - DO: Block Storage or Spaces

### `/app/logs` - Application Logs
- **Purpose**: Application log files
- **Recommendation**: Use container platform logging
  - AWS: CloudWatch Logs
  - GCP: Cloud Logging
  - Azure: Log Analytics

## Security Considerations

### 1. **Secrets Management**
Never hardcode secrets in Dockerfile or docker-compose.yml:

```bash
# ❌ BAD
BLAKE3_KEY=327da54c32bbda6f1b56c2e248620d31324b6bf5664fc31ab4d455f38787a5fa

# ✅ GOOD - Use environment variables or secret managers
BLAKE3_KEY=${BLAKE3_KEY}
```

Use platform-specific secret managers:
- **AWS**: Secrets Manager / Parameter Store
- **GCP**: Secret Manager
- **Azure**: Key Vault
- **DigitalOcean**: App Secrets

### 2. **Non-Root User**
The Dockerfile runs as non-root user `app:app` for security.

### 3. **Network Security**
- Use HTTPS termination at load balancer/proxy level
- Configure firewalls to only expose port 8080 to LB
- Use private networks for database storage

### 4. **Resource Limits**
Set appropriate limits to prevent resource exhaustion:
```yaml
deploy:
  resources:
    limits:
      cpus: '1'
      memory: 512M
    reservations:
      cpus: '0.5'
      memory: 256M
```

## Monitoring & Health Checks

### Health Check Endpoint
```bash
curl http://localhost:8080/health_check
```

Returns `200 OK` if healthy.

### Container Health Check
The Dockerfile includes a built-in health check:
- **Interval**: 30 seconds
- **Timeout**: 3 seconds
- **Retries**: 3
- **Command**: `wget --spider http://localhost:8080/health_check`

## Scaling

### Horizontal Scaling
- Use platform auto-scaling (Kubernetes, ECS, Cloud Run)
- Each instance must have its own database or use shared database
- For SQLite: Use read replicas or migrate to PostgreSQL/MySQL

### Vertical Scaling
- Increase CPU/memory in container definition
- SQLite performs well with increased memory
- Monitor `sqlite` performance logs

## Backup Strategy

### Database Backup
For SQLite, implement regular backups:

```bash
# Add to cron or scheduled task
docker exec email-serv cp /app/data/subscribers.db /backup/subscribers-$(date +%Y%m%d).db
```

Or use the platform's volume snapshot feature.

## Troubleshooting

### Container won't start
```bash
# Check logs
docker logs email-serv

# Check if port is in use
docker ps

# Verify environment variables
docker exec email-serv env
```

### Database permission errors
```bash
# Fix volume permissions
docker exec -u root email-serv chown -R app:app /app/data
```

### High memory usage
- SQLite caches data in memory
- Reduce concurrent connections
- Consider migrating to PostgreSQL/MySQL for high load

## Performance Optimization

### For High Traffic
1. Enable SQLite WAL mode: Set `?journal_mode=WAL` in `DB_CONN`
2. Increase container memory for SQLite cache
3. Use connection pooling (requires code changes)
4. Consider migrating to PostgreSQL/MySQL

### Example: WAL Mode
```yaml
environment:
  - DB_CONN=/app/data/subscribers.db?journal_mode=WAL
```

## Support

For issues or questions:
1. Check container logs: `docker logs email-serv`
2. Review application logs in `/app/logs`
3. Verify health check: `curl http://localhost:8080/health_check`
4. Check resource usage: `docker stats email-serv`