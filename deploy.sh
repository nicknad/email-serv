#!/bin/bash

# Deploy script for email-serv to Docker
# This script builds and deploys the email service

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default values
IMAGE_NAME="email-serv"
CONTAINER_NAME="email-serv"
REGISTRY=""
TAG="latest"

# Functions
print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
    exit 1
}

print_info() {
    echo -e "${YELLOW}ℹ $1${NC}"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --registry)
            REGISTRY="$2"
            shift 2
            ;;
        --tag)
            TAG="$2"
            shift 2
            ;;
        --name)
            IMAGE_NAME="$2"
            shift 2
            ;;
        --help|-h)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --registry <url>   Docker registry URL (e.g., registry.digitalocean.com/user)"
            echo "  --tag <tag>       Image tag (default: latest)"
            echo "  --name <name>      Image name (default: email-serv)"
            echo "  --help, -h         Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                                              # Build and run locally"
            echo "  $0 --registry registry.digitalocean.com/user  # Push to DOCR"
            echo "  $0 --tag v1.0.0 --registry gcr.io/project     # Tag and push to GCR"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            ;;
    esac
done

# Check if Docker is installed
if ! command -v docker &> /dev/null; then
    print_error "Docker is not installed. Please install Docker first."
fi

print_info "Building Docker image: $IMAGE_NAME:$TAG"

# Build the image
docker build -t "$IMAGE_NAME:$TAG" .

if [ $? -eq 0 ]; then
    print_success "Docker image built successfully"
else
    print_error "Failed to build Docker image"
fi

# If registry is specified, push the image
if [ ! -z "$REGISTRY" ]; then
    FULL_IMAGE_NAME="$REGISTRY/$IMAGE_NAME:$TAG"
    print_info "Tagging image: $FULL_IMAGE_NAME"
    
    docker tag "$IMAGE_NAME:$TAG" "$FULL_IMAGE_NAME"
    
    print_info "Pushing image to registry: $FULL_IMAGE_NAME"
    docker push "$FULL_IMAGE_NAME"
    
    if [ $? -eq 0 ]; then
        print_success "Image pushed successfully"
        print_info "Deploy with: docker run -d -p 8080:8080 $FULL_IMAGE_NAME"
    else
        print_error "Failed to push image"
    fi
else
    print_success "Image ready: $IMAGE_NAME:$TAG"
    print_info "To run: docker run -d -p 8080:8080 --name $CONTAINER_NAME --env-file .env -v email_data:/app/data $IMAGE_NAME:$TAG"
    print_info "To use docker-compose: docker-compose up -d"
fi