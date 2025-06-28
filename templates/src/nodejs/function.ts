import {FastifyReply, FastifyRequest} from "fastify";

interface QueryParams {
    name?: string
}

export default {
    function: async (request: FastifyRequest<{ Querystring: QueryParams }>, reply: FastifyReply): Promise<any> => {
        reply.code(201);
        return { message: `${request.query.name} says Hello` }
    },
    // The name of the route/function (AUTO-GENERATED: do not change manually)
    name: '{{ROUTE}}',
};