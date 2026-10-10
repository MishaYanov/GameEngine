import { Component, input, output, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CommonModule } from '@angular/common';
import { AdminApi } from './api';
import type { DashboardSnapshot, Card, CardDraft, BoardColumn, JobRun } from './models';
export function suggestion(card: Card, jobs: JobRun[]): string {
 const last = jobs.find(job => job.id === card.jobs.at(-1));
 if (last?.state === 'FAILED') return 'Latest run failed. Consider moving this card to Blocked.';
 if (last?.state === 'SUCCEEDED') return 'Latest run passed. Review the work before marking Done.';
 return '';
}
@Component({selector: 'admin-board', imports: [FormsModule, CommonModule], templateUrl: './board.html'})
export class Board {
 readonly api = inject(AdminApi); readonly state = input<DashboardSnapshot | null>(null);
 readonly changed = output<void>(); readonly viewLogs = output<{operation_id?: string; session_id?: string}>();
 readonly columns: BoardColumn[] = ['Backlog', 'InProgress', 'Blocked', 'Done'];
 readonly error = signal(''); readonly busy = signal(false); readonly editing = signal<string | null>(null);
 draft: CardDraft = this.blank(); dragging = ''; linkJob = ''; package = 'engine-logger'; profile = 'dev';
 readonly packages = ['game','app','compiler','runtime','renderer','bridge','engine-logger','engine-admin'];
 blank(): CardDraft { return {title:'',description:'',priority:'Normal',component:'',column:'Backlog',session_id:null}; }
 label(column: BoardColumn): string { return column === 'InProgress' ? 'In Progress' : column; }
 cards(column: BoardColumn): Card[] {return (this.state()?.cards ?? []).filter(card => card.column === column);}
 edit(card: Card): void { this.editing.set(card.id); this.draft = {title:card.title,description:card.description,priority:card.priority,component:card.component,column:card.column,session_id:card.session_id}; }
 reset(): void {this.editing.set(null);this.draft=this.blank();}
 async run(command: string, args: Record<string,unknown>): Promise<boolean> {if(this.busy())return false;this.busy.set(true);this.error.set('');try{await this.api.call(command,args);this.changed.emit();return true;}catch(error){this.error.set(String(error));return false;}finally{this.busy.set(false);}}
 async save(): Promise<void> {if(await this.run('save_card',{id:this.editing(),draft:this.draft}))this.reset();}
 move(card: Card, column: BoardColumn, before: string | null = null): void {void this.run('move_card',{id:card.id,column,before});}
 drop(event: DragEvent,column: BoardColumn,before: string | null = null): void {event.preventDefault();event.stopPropagation();const card=this.state()?.cards?.find(card=>card.id===this.dragging);this.dragging='';if(card && card.id!==before)this.move(card,column,before);}
 drag(event: DragEvent,card: Card): void {this.dragging=card.id;event.dataTransfer?.setData('text/plain',card.id);}
 reorder(card: Card,direction: number): void {const cards=this.cards(card.column);const index=cards.findIndex(c=>c.id===card.id);if(direction<0 && index>0)this.move(card,card.column,cards[index-1].id);if(direction>0 && index<cards.length-1)this.move(card,card.column,cards[index+2]?.id ?? null);}
 job(id: string): JobRun | undefined {return this.state()?.jobs?.find(job=>job.id===id);}
 hint(card: Card): string {return suggestion(card,this.state()?.jobs ?? []);}
 queue(card: Card,kind:'BUILD'|'TEST'): void {void this.run('queue_card_job',{id:card.id,kind,package:this.package,profile:this.profile});}
 deleteId = signal<string | null>(null);
 async remove(card: Card): Promise<void> {if(await this.run('delete_card',{id:card.id})){if(this.editing()===card.id)this.reset();this.deleteId.set(null);}}
}
