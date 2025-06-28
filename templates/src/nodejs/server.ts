import Fastify, { FastifyInstance } from 'fastify';
import cors from '@fastify/cors';
import helmet from '@fastify/helmet';
import { env } from 'node:process';
import routes from './function';

// Server configuration
const server = Fastify({
  logger: env['NODE_ENV'] === 'development' ? {
    level: env['LOG_LEVEL'] || 'info',
    transport: {
      target: 'pino-pretty',
      options: {
        colorize: true,
        translateTime: 'HH:MM:ss Z',
        ignore: 'pid,hostname'
      }
    }
  } : {
    level: env['LOG_LEVEL'] || 'info'
  }
});

// Declare server startup function
const start = async (): Promise<void> => {
  try {
    // Register security plugins
    await server.register(helmet, {
      contentSecurityPolicy: {
        directives: {
          defaultSrc: ["'self'"],
          styleSrc: ["'self'", "'unsafe-inline'"],
          scriptSrc: ["'self'"],
          imgSrc: ["'self'", 'data:', 'https:'],
        },
      },
    });

    await server.register(cors, {
      origin: env['CORS_ORIGIN'] ? env['CORS_ORIGIN'].split(',') : true,
      credentials: true
    });

    // Register routes
    registerRoutes(server);

    // Add health check
    server.get('/health', async () => {
      return {
        status: 'ok',
        timestamp: new Date().toISOString(),
        uptime: process.uptime()
      };
    });

    // Start the server
    const port = env['PORT'] || 8080;
    const host = env['HOST'] || '0.0.0.0';

    await server.listen({ port: Number(port), host });

    server.log.info(`Server listening on http://${host}:${port}`);
  } catch (err) {
    server.log.error(err);
    process.exit(1);
  }
};

// Handle graceful shutdown
const gracefulShutdown = async (): Promise<void> => {
  server.log.info('Starting graceful shutdown...');

  try {
    await server.close();
    server.log.info('Server closed successfully');
    process.exit(0);
  } catch (err) {
    server.log.error('Error during shutdown:', err);
    process.exit(1);
  }
};

const registerRoutes = (fastify: FastifyInstance) => {
  fastify.all(`/${routes.name}`, routes.function);
}

// Listen for termination signals
process.on('SIGTERM', gracefulShutdown);
process.on('SIGINT', gracefulShutdown);

// Handle uncaught exceptions
process.on('uncaughtException', (err) => {
  server.log.error('Uncaught Exception:', err);
  process.exit(1);
});

process.on('unhandledRejection', (reason, promise) => {
  server.log.error('Unhandled Rejection at:', promise, 'reason:', reason);
  process.exit(1);
});

// Start the server
start();