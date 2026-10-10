import '@angular/compiler';
import {describe,it,expect} from 'vitest';
import {suggestion} from './board';
import type {Card,JobRun} from './models';

describe('Kanban automation suggestions',()=>{
 const card:Card={id:'card',title:'Fix render',description:'',priority:'High',component:'renderer',column:'InProgress',session_id:null,jobs:['old','new'],updated_ms:0};
 const job=(id:string,state:string):JobRun=>({id,state,kind:'BUILD',package:'renderer',profile:'dev',workspace:'fixture',queued_ms:0,started_ms:1,ended_ms:2,process_id:null,exit_code:0,toolchain:null,artifacts:[],detail:null});
 it('suggests review after latest success while preserving manual status',()=>{expect(suggestion(card,[job('new','SUCCEEDED'),job('old','FAILED')])).toContain('Review');expect(card.column).toBe('InProgress');expect(card.jobs).toEqual(['old','new']);});
 it('suggests blocked after failure and tolerates expired job links',()=>{expect(suggestion(card,[job('new','FAILED')])).toContain('Blocked');expect(suggestion(card,[])).toBe('');});
});
