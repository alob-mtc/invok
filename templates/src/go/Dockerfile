# Stage 1: Build Stage
FROM golang:1.23 as builder

# Set the working directory inside the container
WORKDIR /app

# Copy the specific function package into the container's workspace
# Replace {{FUNCTION}} with the actual function directory or file
COPY . .

# Initialize the Go module (if not already initialized)
RUN go mod init serverless-function

# Download dependencies early to leverage Docker cache
RUN go mod tidy

# Build the Go app
RUN CGO_ENABLED=0 GOOS=linux go build -o main .

# Stage 2: Runtime Stage
FROM gcr.io/distroless/static-debian12

# Set the working directory inside the container
WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/main .

# Expose port 8080
EXPOSE 8080

# Set environment variables (replace with actual environment configurations)
{{ENV}}

# Command to run the application
CMD ["./main"]