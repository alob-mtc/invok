import { FastifyReply, FastifyRequest, HookHandlerDoneFunction } from "fastify";


type InvokHooks = (_request: FastifyRequest, _reply: FastifyReply, done: HookHandlerDoneFunction) => void;
type InvokFunction = (request: FastifyRequest, reply: FastifyReply) => Promise<any>;
interface QueryParams {
    name?: string
}

export default {
    // The name of the route/function (AUTO-GENERATED: do not change manually)
    name: '{{ROUTE}}',
    hooks: [    // You can leave this array empty if you don't need a middleware
        (_request: FastifyRequest, _reply: FastifyReply, done: HookHandlerDoneFunction) => {
            // Middleware code here
            done()
        }
    ] ,
    function: async (request: FastifyRequest<{ Querystring: QueryParams }>, reply: FastifyReply) => {
        reply.code(201);
        return { message: `${request.query.name} says Hello` }
    },
} as { name: string, hooks: InvokHooks[], function: InvokFunction };